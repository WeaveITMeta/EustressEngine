//! The sky under ray tracing, and the sky in AI captures.
//!
//! # Skylight for Solari
//!
//! Solari 0.19 adds nothing for a ray that leaves the scene: ReSTIR GI keeps
//! an empty sample on a miss (`restir_gi.wgsl`), the specular path stops
//! (`specular_gi.wgsl`), and nothing reads the atmosphere, the environment
//! map or `GlobalAmbientLight`. Without a sky, outdoor shade is lit by bounce
//! light alone and comes out far darker than the raster path, whose fill and
//! environment map stand in for the sky.
//!
//! ## Why not an emissive dome
//!
//! An emissive dome around the scene would be sampled as an area light, but
//! in Solari 0.19 it cannot be kept out of the sun's way:
//!
//! - Every traced triangle stops every ray. Each BLAS is built `OPAQUE`
//!   (`scene/blas.rs`), every TLAS instance has mask `0xFF` (`binder.rs`),
//!   and every ray query uses cull mask `0xFF` (`RAY_NO_CULL` in
//!   `raytracing_scene_bindings.wgsl`). No ray type can skip the dome.
//! - A sun shadow ray runs to `RAY_T_MAX`, 100 km (`trace_light_visibility`
//!   in `sampling.wgsl`). From a point `d` from the centre of a dome of
//!   radius `R`, the ray toward the sun leaves the dome up to `asin(d / R)`
//!   from the sun's direction, so an opening around the sun only clears
//!   points within `R sin(opening - sun radius)` of the axis through the
//!   centre. Ground past that is shadowed by the dome's rim, and everything
//!   outside the dome on the side away from the sun lies in its shadow.
//! - Moving the dome past 100 km, where no sun ray reaches, breaks its own
//!   light: a light sample on a triangle is tested with a ray stopped
//!   `RAY_T_MIN` (1 mm) short of it, and an f32 position 100 km out is good
//!   to about 8 mm, so the ray would hit the dome's own triangle about half
//!   the time. The 1 mm margin holds only within about 2 km.
//!
//! ## A hemisphere light that only Solari sees
//!
//! Solari samples a `DirectionalLight` over a cone of directions whose
//! apex angle is the light's `SunDisk::angular_size` (`GpuDirectionalLight::new`
//! in `binder.rs`, `resolve_light_sample` in `sampling.wgsl`), and tests each
//! sample with a ray toward it out to 100 km: a ray to the sky. With an apex
//! angle of 180 degrees the cone is the whole upper hemisphere and the light
//! is a uniform sky. It lights each surface by how much sky it sees, and it
//! blocks nothing, because a light is not geometry. No mesh, so no emissive
//! triangle limit and no acceleration-structure cost.
//!
//! - Only Solari sees it. The raster path keeps, per view, only the
//!   directional lights whose `RenderLayers` meet the view's (`prepare_lights`
//!   in bevy_pbr `render/light.rs`); Solari's binder takes every extracted
//!   directional light (`binder.rs`). The light is on [`TRACED_SKY_LAYER`],
//!   which no camera has, so raster lighting, Bevy's atmosphere (which draws
//!   a sun for each directional light in the view's list) and the AI camera
//!   never see it.
//! - Bevy marks a light visible only when a camera sees its layer, and
//!   extraction drops a light that is not visible (`extract_lights`).
//!   [`keep_traced_sky_light_visible`] marks it, in
//!   `VisibilitySystems::CheckVisibility`, the set Bevy documents for custom
//!   visibility (`SetViewVisibility`).
//! - It has no shadow maps, so Bevy builds and draws no cascades for it.
//!
//! ## Calibration
//!
//! Solari gives the cone a luminance of `color * illuminance / solid angle`,
//! and the solid angle of the hemisphere is 2 pi. Level ground under open sky
//! receives pi times that luminance, half the light's illuminance, so the
//! illuminance is set to twice the skylight wanted on level ground.
//!
//! That skylight is what the engine's sky model already puts there:
//! `SkyLight::sky_lux` (the sun's and the moon's sky lobes, with Brightness,
//! the Sun's switch and the moon's phase in it) plus the night sky's glow,
//! the same totals the exposure adapts to and the raster fill follows. Its
//! hue is the cosine-weighted average of the sky the sky dome draws
//! (`sky_dome_params`, `lobe_radiance`): blue by day, gold at sunset, the
//! blue hour, moonlit blue. `Lighting.EnvironmentDiffuseScale` scales it as
//! it scales the raster fill. Solari multiplies by the same camera exposure
//! as the raster path (`restir_di.wgsl`, `restir_gi.wgsl`) and has no ambient
//! term of its own, so equal light means an equal picture.
//!
//! With the sun overhead the sky puts about 16,300 lux on level ground and
//! the sun 113,000 (130,000 as Solari gets it on the atmosphere path, where
//! the light carries the sun above the air), so shade under open sky is 8 to
//! 9 times darker than sunlight, inside the 6 to 10 times the raster fill
//! was measured to (`SKY_FILL_FRACTION` in `lighting_plugin.rs`).
//!
//! ## Night, stars, clouds
//!
//! The moon is a traced directional light already; the moonlit sky is its
//! lobe in `sky_lux`. The stars' light is the night glow: real starlight is
//! about a thousandth of a lux, and tracing thousands of point lights would
//! spend Solari's light samples (one light is picked uniformly per sample,
//! `generate_random_light_sample`) on nothing visible. Clouds light nothing
//! in the raster path (`volumetric_clouds.rs`: no shadows, not in the
//! environment map) and `SkyLight` does not read them, so they do not change
//! the traced sky either.
//!
//! ## Limits in Solari 0.19
//!
//! - The sky is uniform: shade facing the sun's side of the sky gets the same
//!   skylight as shade facing away. The raster environment map keeps that.
//! - Mirror-smooth surfaces (Solari roughness at or under 0.0225, perceptual
//!   roughness 0.15) show no sky in reflections: a missed ray adds nothing
//!   and a ray cannot hit a light.
//!
//! # The AI camera's sky
//!
//! The AI camera opts out of the atmosphere (`NoAtmosphere`, see
//! `ai_camera.rs`) because Bevy 0.19's atmosphere is single-camera: its
//! `write_atmosphere_buffer` takes `single()` over every camera with an
//! atmosphere and writes nothing when there are two, and a view whose
//! pipelines expect the atmosphere bindings then gets a bind group without
//! them ("bind group descriptor (21) != layout (24)"). So on the atmosphere
//! path its captures had no sky behind the scene, and on every path no
//! skybox and no environment lighting.
//!
//! [`mirror_sky_to_capture_cameras`] gives it the main view's sky without
//! the atmosphere: on the atmosphere path, a `Skybox` of the live sky cubemap
//! Bevy renders for the main camera's environment map (the radiance
//! `render_sky` draws, before exposure); on the cubemap paths, the main
//! camera's own `Skybox`; on every path, the main camera's filtered
//! `EnvironmentMapLight` maps. Nothing is filtered twice and nothing renders
//! while the camera is idle. The sun's disc, the stars, the moon and the
//! clouds are meshes around the main camera that it already draws.
//!
//! [`deferred_prepass_for_capture_cameras`] gives it a `DeferredPrepass`
//! whenever opaque materials default to deferred (`EUSTRESS_SSR=1`, or
//! Solari): a deferred material is never queued in the main pass
//! (`queue_material_meshes` in bevy_pbr `material.rs`), so without the
//! prepass the camera drew no opaque `StandardMaterial` at all.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::camera::visibility::{RenderLayers, SetViewVisibility, VisibilitySystems};
use bevy::camera::Hdr;
use bevy::core_pipeline::prepass::DeferredPrepass;
use bevy::light::{EnvironmentMapLight, GeneratedEnvironmentMapLight, Skybox, SunDisk};
use bevy::material::OpaqueRendererMethod;
use bevy::pbr::DefaultOpaqueRendererMethod;
use bevy::prelude::*;
use bevy::reflect::tuple_struct::TupleStruct;
use eustress_common::plugins::lighting_plugin::{NoAtmosphere, SkyCamera, SkyMode};
use eustress_common::plugins::sky_atmosphere::{luminance, SceneAtmosphere, SkyConfig, SkyExposure, SkyLight, SkyLightSet};
use eustress_common::plugins::sky_dome::{lobe_radiance, sky_dome_params, DomeInputs, Lobe, Perez, PerezSet, SkyDomeParams};
use eustress_common::services::lighting::LightingService;

use super::TraceMarker;
use crate::default_scene::StudioCamera;

// ============================================================================
// Registration
// ============================================================================

/// The traced sky. Called only when ray tracing is active. Generic over the
/// trace marker to match the other ray-tracing parts' call site; the sky is
/// a light, not a traced mesh (see the module docs), so `M` is not used.
pub fn add_systems<M: TraceMarker>(app: &mut App) {
    app.add_systems(
        Update,
        // After this frame's sky light is computed.
        sync_traced_sky_light.after(SkyLightSet).run_if(super::raytracing_active),
    )
    .add_systems(
        PostUpdate,
        keep_traced_sky_light_visible
            .in_set(VisibilitySystems::CheckVisibility)
            .run_if(super::raytracing_active),
    );
}

/// The AI camera's sky and prepass, in every build and mode.
pub fn add_always(app: &mut App) {
    // PostUpdate: after Update's sky attach and Bevy's environment-map
    // systems have applied their commands, before extraction.
    app.add_systems(PostUpdate, (mirror_sky_to_capture_cameras, deferred_prepass_for_capture_cameras));
}

// ============================================================================
// The traced sky light
// ============================================================================

/// The render layer the traced sky light is on. No camera may carry it: a
/// view that did would light its raster pass with the sky twice. Layers 27
/// to 31 are the Player's menus, notices and HUD, the view grid and the
/// Slint overlay.
pub const TRACED_SKY_LAYER: usize = 47;

/// The cone the sky is sampled over, as `SunDisk::angular_size` (the apex
/// angle; Solari halves it): the whole upper hemisphere.
pub const SKY_CONE_ANGLE: f32 = PI;

/// Marks the traced sky light.
#[derive(Component, Debug, Clone, Copy)]
pub struct TracedSkyLight;

/// What the traced sky light carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyFill {
    /// Linear RGB, luminance 1.
    pub color: Vec3,
    /// The light's illuminance, lux: twice the skylight on level ground
    /// (see the module docs).
    pub illuminance: f32,
}

impl SkyFill {
    /// The skylight on unobstructed level ground, lux.
    pub fn level_lux(&self) -> f32 {
        0.5 * self.illuminance
    }
}

/// Rings and segments of the grid the sky's hue is averaged over. The hue
/// changes slowly across the sky; 12 by 24 holds it to well under a percent.
const HUE_RINGS: usize = 12;
const HUE_SEGMENTS: usize = 24;

/// The Perez distributions the dome's parameters were built with.
fn perez_set(params: &SkyDomeParams) -> PerezSet {
    let perez = |abcd: Vec4, e: f32| Perez { a: abcd.x, b: abcd.y, c: abcd.z, d: abcd.w, e };
    PerezSet {
        luminance: perez(params.perez_lum, params.perez_e.x),
        x: perez(params.perez_x, params.perez_e.y),
        y: perez(params.perez_y, params.perez_e.z),
    }
}

/// The night glow's light on level ground, lux. The dome draws it as
/// `glow (1 + h (1 - cos theta)^2)`, which puts `pi glow (1 + h / 6)` there.
pub fn night_glow_lux(params: &SkyDomeParams) -> f32 {
    PI * luminance(params.night_sky.truncate()) * (1.0 + params.night_sky.w / 6.0)
}

/// Light on level ground from the sky the dome draws above the horizon
/// (both lobes and the night glow, not the stars), linear RGB, lux.
pub fn sky_irradiance(params: &SkyDomeParams) -> Vec3 {
    let perez = perez_set(params);
    let sun = Lobe { toward: params.sun.truncate(), zenith: params.sun_zenith, tint: params.sun_tint.truncate() };
    let moon = Lobe { toward: params.moon.truncate(), zenith: params.moon_zenith, tint: params.moon_tint.truncate() };
    let glow = params.night_sky.truncate();
    let horizon = params.night_sky.w;
    let (d_theta, d_phi) = (FRAC_PI_2 / HUE_RINGS as f32, TAU / HUE_SEGMENTS as f32);
    let mut sum = Vec3::ZERO;
    for i in 0..HUE_RINGS {
        let theta = (i as f32 + 0.5) * d_theta;
        let (sin_t, cos_t) = theta.sin_cos();
        let low = 1.0 - cos_t;
        let glow_here = glow * (1.0 + horizon * low * low);
        for j in 0..HUE_SEGMENTS {
            let phi = (j as f32 + 0.5) * d_phi;
            let ray = Vec3::new(sin_t * phi.cos(), cos_t, sin_t * phi.sin());
            let radiance = lobe_radiance(&perez, &sun, ray) + lobe_radiance(&perez, &moon, ray) + glow_here;
            sum += radiance * cos_t * sin_t;
        }
    }
    sum * d_theta * d_phi
}

/// The traced sky for this sky: the level-ground total from `SkyLight`
/// and the night glow, the hue of the sky the dome draws, scaled by
/// `diffuse_scale` (`Lighting.EnvironmentDiffuseScale`).
pub fn traced_sky_fill(sky_light: &SkyLight, params: &SkyDomeParams, diffuse_scale: f32) -> SkyFill {
    let rgb = sky_irradiance(params);
    let share = luminance(rgb);
    let color = if share > 1e-9 { rgb / share } else { Vec3::ONE };
    let level = (sky_light.sky_lux.max(0.0) + night_glow_lux(params)) * diffuse_scale.max(0.0);
    SkyFill { color, illuminance: 2.0 * level }
}

/// The inputs the fill is computed from, to skip the work while they hold
/// still: the dome's parameters cost a few thousand Perez evaluations.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FillKey {
    sun: Vec3,
    moon: Vec3,
    sun_light: Vec3,
    moon_light: Vec3,
    sky_lux: f32,
    density: f32,
    haze: f32,
    decay: [f32; 4],
    diffuse_scale: f32,
}

/// A twentieth of a degree: the sun moves that far in about five seconds
/// of a ten-minute day.
const KEY_ANGLE_COS: f32 = 0.999_999_6;
/// Half a percent of light.
const KEY_RELATIVE: f32 = 0.005;

impl FillKey {
    fn of(sky_light: &SkyLight, scene: &SceneAtmosphere, diffuse_scale: f32) -> Self {
        let a = &scene.atmosphere;
        Self {
            sun: sky_light.sun_direction,
            moon: sky_light.moon_direction,
            sun_light: sky_light.sun_light,
            moon_light: sky_light.moon_light,
            sky_lux: sky_light.sky_lux,
            density: a.density,
            haze: a.haze,
            decay: a.decay,
            diffuse_scale,
        }
    }

    /// Close enough that the fill would not visibly change.
    fn holds(&self, last: &Self) -> bool {
        let same_dir = |a: Vec3, b: Vec3| a.dot(b) >= KEY_ANGLE_COS * a.length() * b.length();
        let same = |a: f32, b: f32| (a - b).abs() <= KEY_RELATIVE * a.abs().max(b.abs()) + 1e-6;
        let same3 = |a: Vec3, b: Vec3| same(a.x, b.x) && same(a.y, b.y) && same(a.z, b.z);
        same_dir(self.sun, last.sun)
            && same_dir(self.moon, last.moon)
            && same3(self.sun_light, last.sun_light)
            && same3(self.moon_light, last.moon_light)
            && same(self.sky_lux, last.sky_lux)
            && self.density == last.density
            && self.haze == last.haze
            && self.decay == last.decay
            && self.diffuse_scale == last.diffuse_scale
    }
}

/// The light's components, made once.
fn traced_sky_light_bundle(fill: SkyFill) -> impl Bundle {
    (
        DirectionalLight {
            color: Color::linear_rgb(fill.color.x, fill.color.y, fill.color.z),
            illuminance: fill.illuminance,
            // Solari traces its visibility; maps would be drawn for nothing.
            shadow_maps_enabled: false,
            contact_shadows_enabled: false,
            ..default()
        },
        // Intensity 0: no disc, should a view ever list it.
        SunDisk { angular_size: SKY_CONE_ANGLE, intensity: 0.0 },
        // From straight up: the cone's axis is the zenith.
        Transform::default().looking_to(Vec3::NEG_Y, Vec3::Z),
        RenderLayers::layer(TRACED_SKY_LAYER),
        TracedSkyLight,
        Name::new("Traced Sky Light"),
    )
}

/// Spawn the traced sky light, and keep its colour and strength with the
/// sky. Writes only when the sky moved enough to show.
#[allow(clippy::too_many_arguments)]
fn sync_traced_sky_light(
    mut commands: Commands,
    sky_light: Option<Res<SkyLight>>,
    scene: Option<Res<SceneAtmosphere>>,
    lighting: Option<Res<LightingService>>,
    config: Option<Res<SkyConfig>>,
    exposure: Option<Res<SkyExposure>>,
    mut lights: Query<(Entity, &mut DirectionalLight), With<TracedSkyLight>>,
    mut last: Local<Option<FillKey>>,
) {
    let (Some(sky_light), Some(scene), Some(config), Some(exposure)) = (sky_light, scene, config, exposure) else {
        return;
    };
    let diffuse_scale = lighting.map_or(1.0, |l| l.environment_diffuse_scale);
    let key = FillKey::of(&sky_light, &scene, diffuse_scale);
    let mut found = lights.iter_mut();
    let existing = found.next().map(|(_, light)| light);
    // One sky light: a second would light the scene twice.
    for (extra, _) in found {
        commands.entity(extra).despawn();
    }
    if existing.is_some() && last.as_ref().is_some_and(|l| key.holds(l)) {
        return;
    }
    *last = Some(key);

    let params = sky_dome_params(&DomeInputs {
        // Only the lobes, the Perez shape and the night glow are read.
        mode: SkyMode::Gradient,
        atmosphere: &scene.atmosphere,
        sky_light: &sky_light,
        exposure: &exposure,
        config: &config,
        disc_radius: None,
        shown: false,
        star_rotation: Quat::IDENTITY,
    });
    let fill = traced_sky_fill(&sky_light, &params, diffuse_scale);

    let Some(mut light) = existing else {
        commands.spawn(traced_sky_light_bundle(fill));
        info!(
            "Ray tracing: sky light ready ({:.0} lux on level ground, layer {TRACED_SKY_LAYER}, seen by Solari only)",
            fill.level_lux()
        );
        return;
    };
    let color = Color::linear_rgb(fill.color.x, fill.color.y, fill.color.z);
    if light.color != color || (light.illuminance - fill.illuminance).abs() > 1e-3 * fill.illuminance.max(1e-3) {
        light.color = color;
        light.illuminance = fill.illuminance;
    }
}

/// No camera sees the sky light's layer, so Bevy never marks it visible and
/// extraction would drop it before Solari's binder looks. Runs in
/// `CheckVisibility`, after Bevy resets visibility for the frame.
pub fn keep_traced_sky_light_visible(mut lights: Query<&mut ViewVisibility, With<TracedSkyLight>>) {
    for mut visibility in &mut lights {
        visibility.set_visible();
    }
}

// ============================================================================
// The AI camera's sky
// ============================================================================

/// `Skybox::brightness` for the live atmosphere cubemap: it holds the
/// radiance `render_sky` draws before it multiplies by the exposure, and the
/// skybox multiplies by the capture camera's exposure, which follows the
/// main camera's.
pub const ATMOSPHERE_SKYBOX_BRIGHTNESS: f32 = 1.0;

/// The skybox a capture camera shows for a main camera on sky path `mode`.
///
/// On the atmosphere path the main camera's `Skybox` holds the star field at
/// brightness 0 (the dome draws the stars), and the sky is Bevy's
/// atmosphere, rendered each frame into the cubemap behind its environment
/// map. On the cubemap paths the main camera's `Skybox` is the sky.
pub fn capture_skybox(
    mode: SkyMode,
    skybox: Option<&Skybox>,
    generated: Option<&GeneratedEnvironmentMapLight>,
) -> Option<Skybox> {
    match mode {
        SkyMode::Atmosphere => generated.map(|g| Skybox {
            image: Some(g.environment_map.clone()),
            brightness: ATMOSPHERE_SKYBOX_BRIGHTNESS,
            rotation: g.rotation,
        }),
        SkyMode::Skybox | SkyMode::Gradient => skybox.filter(|s| s.image.is_some()).cloned(),
    }
}

fn same_skybox(a: &Skybox, b: &Skybox) -> bool {
    a.image == b.image && a.brightness == b.brightness && a.rotation == b.rotation
}

fn same_environment(a: &EnvironmentMapLight, b: &EnvironmentMapLight) -> bool {
    a.diffuse_map == b.diffuse_map
        && a.specular_map == b.specular_map
        && a.intensity == b.intensity
        && a.rotation == b.rotation
        && a.affects_lightmapped_mesh_diffuse == b.affects_lightmapped_mesh_diffuse
}

type MainSky = (
    &'static Camera,
    &'static SkyCamera,
    Option<&'static Skybox>,
    Option<&'static GeneratedEnvironmentMapLight>,
    Option<&'static EnvironmentMapLight>,
    Has<Hdr>,
);

/// A Studio camera that skips the sky: the AI capture camera. Not a sky
/// camera, so disjoint from [`MainSky`] by `NoAtmosphere`.
type CaptureCamera = (With<Camera3d>, With<StudioCamera>, With<NoAtmosphere>, Without<SkyCamera>);

/// Give every capture camera the main view's sky and environment lighting:
/// the sky camera the sky dome follows (active, lowest order), copied without
/// its atmosphere, so there is still one atmosphere camera.
pub fn mirror_sky_to_capture_cameras(
    mut commands: Commands,
    sources: Query<MainSky, (With<Camera3d>, Without<NoAtmosphere>)>,
    mut captures: Query<
        (Entity, Option<&mut Skybox>, Option<&mut EnvironmentMapLight>, Has<Hdr>),
        CaptureCamera,
    >,
) {
    let main = sources
        .iter()
        .filter(|(camera, ..)| camera.is_active)
        .min_by_key(|(camera, ..)| camera.order);
    let (sky, environment, hdr) = match main {
        Some((_, sky_camera, skybox, generated, environment, hdr)) => {
            (capture_skybox(sky_camera.mode, skybox, generated), environment.cloned(), hdr)
        }
        // No main view yet (a Space opening): leave the capture camera as it is.
        None => return,
    };
    for (entity, mut current_sky, mut current_environment, has_hdr) in &mut captures {
        match (&sky, current_sky.as_deref_mut()) {
            (Some(wanted), Some(current)) => {
                if !same_skybox(wanted, current) {
                    *current = wanted.clone();
                }
            }
            (Some(wanted), None) => {
                commands.entity(entity).insert(wanted.clone());
            }
            (None, Some(_)) => {
                commands.entity(entity).remove::<Skybox>();
            }
            (None, None) => {}
        }
        match (&environment, current_environment.as_deref_mut()) {
            (Some(wanted), Some(current)) => {
                if !same_environment(wanted, current) {
                    *current = wanted.clone();
                }
            }
            (Some(wanted), None) => {
                commands.entity(entity).insert(wanted.clone());
            }
            (None, Some(_)) => {
                commands.entity(entity).remove::<EnvironmentMapLight>();
            }
            (None, None) => {}
        }
        // The sky tonemapped as the main view tonemaps it. Only added: bloom
        // requires Hdr, and taking it away would leave bloom without it.
        if hdr && !has_hdr {
            commands.entity(entity).insert(Hdr);
        }
    }
}

/// Whether opaque materials default to deferred. The field is private;
/// the resource derives `Reflect`, which reads it.
pub fn is_deferred(method: &DefaultOpaqueRendererMethod) -> bool {
    method
        .field(0)
        .and_then(|field| field.try_downcast_ref::<OpaqueRendererMethod>())
        .is_some_and(|m| *m == OpaqueRendererMethod::Deferred)
}

/// A deferred material draws only through a `DeferredPrepass` (bevy_pbr
/// never queues it in the main pass), so a capture camera gets one whenever
/// deferred is the default: SSR and Solari both set that, for the whole app,
/// at startup.
pub fn deferred_prepass_for_capture_cameras(
    mut commands: Commands,
    method: Option<Res<DefaultOpaqueRendererMethod>>,
    cameras: Query<Entity, (CaptureCamera, Without<DeferredPrepass>)>,
) {
    if !method.as_deref().is_some_and(is_deferred) {
        return;
    }
    for camera in &cameras {
        commands.entity(camera).insert(DeferredPrepass);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{RaytracingLaunch, RaytracingStatus};
    use bevy::asset::AssetPlugin;
    use eustress_common::plugins::sky_atmosphere::clear_sky_lux;

    // ---- a stand-in trace marker, as the core tests use

    #[derive(Component)]
    struct TestTraced(Handle<Mesh>);

    impl TraceMarker for TestTraced {
        fn new(mesh: Handle<Mesh>) -> Self {
            Self(mesh)
        }
        fn mesh_id(&self) -> AssetId<Mesh> {
            self.0.id()
        }
    }

    // ---- sky states

    const RAW_SUNLIGHT: f32 = bevy::light::light_consts::lux::RAW_SUNLIGHT;

    /// The sky with the sun at `elevation_deg` toward -Z and no moon, as
    /// `update_sky_light` computes it for a clear Earth sky (transmittance
    /// left at one: these tests are about the sky, not the sun).
    fn sky_at(elevation_deg: f32) -> SkyLight {
        let e = elevation_deg.to_radians();
        let sun = Vec3::new(0.0, e.sin(), -e.cos());
        let sun_light = Vec3::splat(RAW_SUNLIGHT);
        SkyLight {
            sun_direction: sun,
            moon_direction: Vec3::NEG_Y,
            moon_lit_from: Vec3::Y,
            sun_light,
            moon_light: Vec3::ZERO,
            sun_ground: sun_light,
            sun_disc_light: sun_light,
            moon_ground: Vec3::ZERO,
            sky_lux: clear_sky_lux(elevation_deg),
            night: ((2.0 - elevation_deg) / 12.0).clamp(0.0, 1.0),
            key_lux: 70_000.0,
        }
    }

    fn params_for(sky: &SkyLight) -> SkyDomeParams {
        sky_dome_params(&DomeInputs {
            mode: SkyMode::Gradient,
            atmosphere: &SceneAtmosphere::default().atmosphere,
            sky_light: sky,
            exposure: &SkyExposure::default(),
            config: &SkyConfig::default(),
            disc_radius: None,
            shown: false,
            star_rotation: Quat::IDENTITY,
        })
    }

    fn fill_at(elevation_deg: f32) -> SkyFill {
        let sky = sky_at(elevation_deg);
        traced_sky_fill(&sky, &params_for(&sky), 1.0)
    }

    // ---- an app with the ray-tracing sky registered

    fn rt_app(status: RaytracingStatus) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .insert_resource(RaytracingLaunch::for_status(status))
            .insert_resource(sky_at(45.0))
            .init_resource::<SceneAtmosphere>()
            .init_resource::<LightingService>()
            .init_resource::<SkyConfig>()
            .init_resource::<SkyExposure>();
        add_systems::<TestTraced>(&mut app);
        add_always(&mut app);
        app
    }

    fn sky_lights(app: &mut App) -> Vec<Entity> {
        let mut q = app.world_mut().query_filtered::<Entity, With<TracedSkyLight>>();
        q.iter(app.world()).collect()
    }

    fn the_light(app: &mut App) -> (DirectionalLight, SunDisk, Transform, RenderLayers, ViewVisibility) {
        let lights = sky_lights(app);
        assert_eq!(lights.len(), 1, "expected exactly one traced sky light");
        let e = app.world().entity(lights[0]);
        (
            *e.get::<DirectionalLight>().unwrap(),
            e.get::<SunDisk>().unwrap().clone(),
            *e.get::<Transform>().unwrap(),
            e.get::<RenderLayers>().unwrap().clone(),
            *e.get::<ViewVisibility>().unwrap(),
        )
    }

    // ---- the traced sky light

    #[test]
    fn the_sky_light_is_a_hemisphere_only_solari_sees() {
        let mut app = rt_app(RaytracingStatus::Active);
        app.update();
        app.update();
        let (light, disk, transform, layers, visibility) = the_light(&mut app);
        assert_eq!(disk.angular_size, PI, "the cone is the upper hemisphere");
        assert_eq!(disk.intensity, 0.0);
        assert!(transform.back().dot(Vec3::Y) > 0.9999, "toward-light is the zenith: {:?}", transform.back());
        assert!(!light.shadow_maps_enabled && !light.contact_shadows_enabled);
        // No camera layer the engine uses meets it: the default (0), the
        // Player's menu, notice and HUD layers, the view grid, the overlay.
        for used in [0usize, 27, 28, 29, 30, 31] {
            assert!(!layers.intersects(&RenderLayers::layer(used)), "sky light shares layer {used}");
        }
        assert!(!layers.intersects(&RenderLayers::default()));
        assert!(visibility.get(), "extraction drops a light that is not view-visible");
        // A light, not geometry: nothing traced, nothing drawn, so nothing
        // that could block the sun or count against the emissive limit.
        let mut traced = app.world_mut().query::<&TestTraced>();
        assert_eq!(traced.iter(app.world()).count(), 0);
        let mut meshes = app.world_mut().query::<&Mesh3d>();
        assert_eq!(meshes.iter(app.world()).count(), 0);
    }

    /// Solari's own arithmetic for a directional light (`GpuDirectionalLight::new`
    /// in binder.rs and the cone sampling in sampling.wgsl, restated), used to
    /// integrate what the light puts on a surface.
    fn solari_irradiance(color: Vec3, illuminance: f32, angular_size: f32, toward: Vec3, normal: Vec3) -> Vec3 {
        let cos_theta_max = (angular_size / 2.0).cos();
        let solid_angle = TAU * (1.0 - cos_theta_max);
        let luminance = color * illuminance / solid_angle;
        // Stratified u, v; each sample weighs inverse_pdf = solid_angle.
        let n = 256;
        let basis = Mat3::from_quat(Quat::from_rotation_arc(Vec3::Z, toward));
        let mut sum = Vec3::ZERO;
        for i in 0..n {
            for j in 0..n {
                let (u, v) = ((i as f32 + 0.5) / n as f32, (j as f32 + 0.5) / n as f32);
                let cos_theta = (1.0 - u) + u * cos_theta_max;
                let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
                let phi = v * TAU;
                let dir = basis * Vec3::new(phi.cos() * sin_theta, phi.sin() * sin_theta, cos_theta);
                sum += luminance * dir.dot(normal).max(0.0) * solid_angle;
            }
        }
        sum / (n * n) as f32
    }

    #[test]
    fn solari_reads_it_as_the_engines_sky_on_level_ground() {
        let mut app = rt_app(RaytracingStatus::Active);
        app.update();
        let (light, disk, transform, ..) = the_light(&mut app);
        let sky = sky_at(45.0);
        let wanted = sky.sky_lux + night_glow_lux(&params_for(&sky));
        let c = light.color.to_linear();
        let color = Vec3::new(c.red, c.green, c.blue);
        let toward = transform.back().as_vec3();
        let level = luminance(solari_irradiance(color, light.illuminance, disk.angular_size, toward, Vec3::Y));
        assert!((level - wanted).abs() < 0.01 * wanted, "level ground gets {level} lux, the sky gives {wanted}");
        // A uniform sky: a wall sees half of it.
        let wall = luminance(solari_irradiance(color, light.illuminance, disk.angular_size, toward, Vec3::X));
        assert!((wall - 0.5 * wanted).abs() < 0.02 * wanted, "a wall gets {wall}, half the sky is {}", 0.5 * wanted);
        // A floor's underside sees none of it (the ground's light is bounce).
        let under = luminance(solari_irradiance(color, light.illuminance, disk.angular_size, toward, Vec3::NEG_Y));
        assert!(under < 1e-3 * wanted, "the sky lit a downward face: {under}");
    }

    #[test]
    fn the_fill_follows_the_sky() {
        let noon = fill_at(70.0);
        let low = fill_at(4.0);
        let dusk = fill_at(-5.0);
        // The level each time is the engine's own sky light plus the glow.
        for (elevation, fill) in [(70.0, noon), (4.0, low), (-5.0, dusk)] {
            let sky = sky_at(elevation);
            let wanted = sky.sky_lux + night_glow_lux(&params_for(&sky));
            assert!((fill.level_lux() - wanted).abs() < 1e-3 * wanted, "{elevation}: {} vs {wanted}", fill.level_lux());
            assert!((luminance(fill.color) - 1.0).abs() < 1e-3, "colour carries no strength");
        }
        assert!(noon.level_lux() > low.level_lux() && low.level_lux() > dusk.level_lux());
        // A clear noon sky is blue; a low sun's sky is warmer; dusk turns blue.
        assert!(noon.color.z > noon.color.x, "noon sky not blue: {:?}", noon.color);
        let warmth = |f: SkyFill| f.color.x / f.color.z;
        assert!(warmth(low) > warmth(noon), "low sun {:?} not warmer than noon {:?}", low.color, noon.color);
        assert!(warmth(dusk) < warmth(low), "dusk {:?} not bluer than the low sun {:?}", dusk.color, low.color);
        // Night: the glow floor carries it, in the glow's blue.
        let night = fill_at(-30.0);
        let glow = night_glow_lux(&params_for(&sky_at(-30.0)));
        assert!(glow > 1.0 && (night.level_lux() - glow).abs() < 0.01 * glow + 1e-3);
        assert!(night.color.z > night.color.x);
        // EnvironmentDiffuseScale and Brightness reach it.
        let sky = sky_at(45.0);
        let half = traced_sky_fill(&sky, &params_for(&sky), 0.5);
        assert!((half.illuminance - 0.5 * fill_at(45.0).illuminance).abs() < 1e-3 * half.illuminance);
        let mut bright = sky_at(45.0);
        bright.sun_light *= 2.0;
        bright.sky_lux *= 2.0;
        let doubled = traced_sky_fill(&bright, &params_for(&bright), 1.0);
        assert!(doubled.level_lux() > 1.9 * fill_at(45.0).level_lux());
    }

    #[test]
    fn the_hue_is_the_skys_average() {
        // The grid average agrees with the dome's own normalisation: the
        // two lobes and the glow put about sky_lux + glow on level ground,
        // by day, at dusk and at night (the glow alone).
        for elevation in [70.0, 20.0, 4.0, -3.0, -6.0, -30.0] {
            let sky = sky_at(elevation);
            let params = params_for(&sky);
            let drawn = luminance(sky_irradiance(&params));
            let wanted = sky.sky_lux + night_glow_lux(&params);
            assert!((drawn - wanted).abs() < 0.03 * wanted, "{elevation}: the dome draws {drawn}, the sky light is {wanted}");
        }
    }

    #[derive(Resource, Default)]
    struct Writes(u32);

    fn count_writes(mut writes: ResMut<Writes>, changed: Query<(), (With<TracedSkyLight>, Changed<DirectionalLight>)>) {
        writes.0 += changed.iter().count() as u32;
    }

    #[test]
    fn the_light_follows_the_sky_and_is_left_alone_when_it_holds() {
        let mut app = rt_app(RaytracingStatus::Active);
        app.init_resource::<Writes>().add_systems(Last, count_writes);
        app.update();
        app.update();
        let first = the_light(&mut app).0;
        let before = app.world().resource::<Writes>().0;
        // update_sky_light rewrites SkyLight every frame; the same sky must
        // not touch the light.
        for _ in 0..5 {
            let same = sky_at(45.0);
            app.insert_resource(same);
            app.update();
        }
        assert_eq!(app.world().resource::<Writes>().0, before, "an unchanged sky wrote the light");
        // A sun a hundredth of a degree away is not worth a write either.
        app.insert_resource(sky_at(45.01));
        app.update();
        assert_eq!(app.world().resource::<Writes>().0, before, "a negligible move wrote the light");
        // Sunset is.
        app.insert_resource(sky_at(3.0));
        app.update();
        let after = the_light(&mut app).0;
        assert!(app.world().resource::<Writes>().0 > before);
        assert!(after.illuminance < 0.6 * first.illuminance, "{} vs {}", after.illuminance, first.illuminance);
        assert_ne!(after.color, first.color);
        // Gone (a Space reset that despawns it): put back.
        let lights = sky_lights(&mut app);
        app.world_mut().despawn(lights[0]);
        app.update();
        assert_eq!(sky_lights(&mut app).len(), 1);
        // A second one would light the scene twice: one is kept.
        let fill = fill_at(3.0);
        app.world_mut().spawn(traced_sky_light_bundle(fill));
        app.update();
        app.update();
        assert_eq!(sky_lights(&mut app).len(), 1, "two sky lights");
    }

    #[test]
    fn nothing_traced_unless_ray_tracing_is_active() {
        for status in [RaytracingStatus::Off, RaytracingStatus::NotInBuild, RaytracingStatus::Unsupported] {
            let mut app = rt_app(status);
            app.update();
            app.update();
            assert!(sky_lights(&mut app).is_empty(), "{status:?} spawned a sky light");
        }
        let mut app = rt_app(RaytracingStatus::Active);
        app.world_mut().remove_resource::<RaytracingLaunch>();
        app.update();
        assert!(sky_lights(&mut app).is_empty(), "no launch decision spawned a sky light");
    }

    // ---- the AI camera's sky

    fn capture_app(method: Option<DefaultOpaqueRendererMethod>) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default())).init_asset::<Image>();
        if let Some(method) = method {
            app.insert_resource(method);
        }
        add_always(&mut app);
        app
    }

    fn image(app: &mut App) -> Handle<Image> {
        app.world_mut().resource_mut::<Assets<Image>>().add(Image::default())
    }

    struct MainView {
        camera: Entity,
        live_sky: Handle<Image>,
        stars: Handle<Image>,
        diffuse: Handle<Image>,
        specular: Handle<Image>,
    }

    /// The editor camera on the atmosphere path, as the sky plugin and
    /// Bevy's atmosphere leave it: the star field at brightness 0, the live
    /// sky cubemap behind a generated environment map, and its filtered maps.
    fn main_view(app: &mut App, mode: SkyMode) -> MainView {
        let (live_sky, stars, diffuse, specular) = (image(app), image(app), image(app), image(app));
        let camera = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Camera { order: 0, ..default() },
                StudioCamera,
                SkyCamera { mode },
                Hdr,
                Skybox { image: Some(stars.clone()), brightness: 0.0, rotation: Quat::IDENTITY },
                GeneratedEnvironmentMapLight {
                    environment_map: live_sky.clone(),
                    intensity: 0.8,
                    rotation: Quat::IDENTITY,
                    affects_lightmapped_mesh_diffuse: false,
                },
                EnvironmentMapLight {
                    diffuse_map: diffuse.clone(),
                    specular_map: specular.clone(),
                    intensity: 0.8,
                    rotation: Quat::IDENTITY,
                    affects_lightmapped_mesh_diffuse: false,
                },
            ))
            .id();
        MainView { camera, live_sky, stars, diffuse, specular }
    }

    /// The AI camera's shape: a Studio camera that skips the sky, off, at
    /// order -1, without Hdr (as with `EUSTRESS_BLOOM=0`).
    fn ai_camera(app: &mut App) -> Entity {
        app.world_mut()
            .spawn((Camera3d::default(), Camera { order: -1, is_active: false, ..default() }, StudioCamera, NoAtmosphere))
            .id()
    }

    /// The Slint overlay's shape: skips the sky, not a Studio camera.
    fn overlay_camera(app: &mut App) -> Entity {
        app.world_mut().spawn((Camera3d::default(), Camera { order: 1, ..default() }, NoAtmosphere)).id()
    }

    #[test]
    fn the_ai_camera_shows_the_live_atmosphere_sky() {
        let mut app = capture_app(None);
        let main = main_view(&mut app, SkyMode::Atmosphere);
        let ai = ai_camera(&mut app);
        let overlay = overlay_camera(&mut app);
        app.update();
        let e = app.world().entity(ai);
        let sky = e.get::<Skybox>().expect("no skybox on the AI camera");
        assert_eq!(sky.image, Some(main.live_sky.clone()), "not the live sky cubemap");
        assert_ne!(sky.image, Some(main.stars.clone()), "the dark star field is not the sky");
        assert_eq!(sky.brightness, ATMOSPHERE_SKYBOX_BRIGHTNESS);
        let env = e.get::<EnvironmentMapLight>().expect("no environment light");
        assert_eq!((env.diffuse_map.clone(), env.specular_map.clone()), (main.diffuse.clone(), main.specular.clone()));
        assert_eq!(env.intensity, 0.8);
        assert!(e.contains::<Hdr>(), "tonemapped unlike the main view");
        // Still not an atmosphere camera, and nothing filtered twice.
        assert!(!e.contains::<GeneratedEnvironmentMapLight>());
        assert!(!e.contains::<SkyCamera>());
        // The overlay draws no sky.
        assert!(!app.world().entity(overlay).contains::<Skybox>());
        assert!(!app.world().entity(overlay).contains::<EnvironmentMapLight>());
        // The main camera is untouched.
        assert_eq!(app.world().get::<Skybox>(main.camera).unwrap().image, Some(main.stars));
    }

    #[test]
    fn the_ai_camera_follows_a_cubemap_sky_and_its_changes() {
        let mut app = capture_app(None);
        let main = main_view(&mut app, SkyMode::Gradient);
        let gradient = image(&mut app);
        app.world_mut()
            .entity_mut(main.camera)
            .insert(Skybox { image: Some(gradient.clone()), brightness: 2.5, rotation: Quat::IDENTITY });
        let ai = ai_camera(&mut app);
        app.update();
        let sky = app.world().get::<Skybox>(ai).unwrap();
        assert_eq!((sky.image.clone(), sky.brightness), (Some(gradient.clone()), 2.5));
        // The sky brightens with the day.
        app.world_mut().get_mut::<Skybox>(main.camera).unwrap().brightness = 4.0;
        app.update();
        assert_eq!(app.world().get::<Skybox>(ai).unwrap().brightness, 4.0);
        // The main view loses its environment map (a sky path switch): so
        // does the capture.
        app.world_mut().entity_mut(main.camera).remove::<EnvironmentMapLight>();
        app.update();
        assert!(app.world().get::<EnvironmentMapLight>(ai).is_none(), "stale environment map kept");
        // An atmosphere path whose cubemap is not made yet: no sky rather
        // than the star field.
        app.world_mut().entity_mut(main.camera).insert(SkyCamera { mode: SkyMode::Atmosphere });
        app.world_mut().entity_mut(main.camera).remove::<GeneratedEnvironmentMapLight>();
        app.update();
        assert!(app.world().get::<Skybox>(ai).is_none(), "kept a sky the main view no longer has");
    }

    #[test]
    fn the_sky_comes_from_the_view_the_dome_follows() {
        let mut app = capture_app(None);
        // An inactive sky camera at order 0 and the active one at order 2.
        let idle = main_view(&mut app, SkyMode::Atmosphere);
        app.world_mut().get_mut::<Camera>(idle.camera).unwrap().is_active = false;
        let live = main_view(&mut app, SkyMode::Atmosphere);
        app.world_mut().get_mut::<Camera>(live.camera).unwrap().order = 2;
        let ai = ai_camera(&mut app);
        app.update();
        assert_eq!(app.world().get::<Skybox>(ai).unwrap().image, Some(live.live_sky.clone()));
        // Two active: the lowest order wins, as for the sky dome.
        let first = main_view(&mut app, SkyMode::Atmosphere);
        app.world_mut().get_mut::<Camera>(first.camera).unwrap().order = 1;
        app.update();
        assert_eq!(app.world().get::<Skybox>(ai).unwrap().image, Some(first.live_sky));
    }

    #[test]
    fn deferred_by_default_gives_capture_cameras_the_prepass() {
        assert!(is_deferred(&DefaultOpaqueRendererMethod::deferred()));
        assert!(!is_deferred(&DefaultOpaqueRendererMethod::forward()));
        assert!(!is_deferred(&DefaultOpaqueRendererMethod::default()));

        for (method, wanted) in [
            (Some(DefaultOpaqueRendererMethod::deferred()), true),
            (Some(DefaultOpaqueRendererMethod::forward()), false),
            (None, false),
        ] {
            let mut app = capture_app(method.clone());
            let main = main_view(&mut app, SkyMode::Atmosphere);
            let ai = ai_camera(&mut app);
            let overlay = overlay_camera(&mut app);
            app.update();
            assert_eq!(app.world().get::<DeferredPrepass>(ai).is_some(), wanted, "{method:?}");
            assert!(app.world().get::<DeferredPrepass>(overlay).is_none(), "the overlay draws no scene");
            assert!(app.world().get::<DeferredPrepass>(main.camera).is_none(), "the main camera is SSR's or Solari's");
        }
    }

    #[test]
    fn the_ai_camera_gets_its_sky_in_both_modes() {
        for status in [RaytracingStatus::Off, RaytracingStatus::Active] {
            let mut app = rt_app(status);
            // Solari makes deferred the default when it is active.
            if status.is_active() {
                app.insert_resource(DefaultOpaqueRendererMethod::deferred());
            }
            let main = main_view(&mut app, SkyMode::Atmosphere);
            let ai = ai_camera(&mut app);
            app.update();
            app.update();
            let e = app.world().entity(ai);
            assert_eq!(e.get::<Skybox>().unwrap().image, Some(main.live_sky.clone()), "{status:?}");
            assert!(e.contains::<EnvironmentMapLight>(), "{status:?}");
            assert_eq!(e.contains::<DeferredPrepass>(), status.is_active(), "{status:?}");
            // The traced sky light never reaches the AI camera's raster pass.
            let ai_layers = e.get::<RenderLayers>().cloned().unwrap_or_default();
            for light in sky_lights(&mut app) {
                let layers = app.world().get::<RenderLayers>(light).unwrap();
                assert!(!layers.intersects(&ai_layers), "{status:?}: the AI camera sees the traced sky");
            }
            assert_eq!(sky_lights(&mut app).len(), usize::from(status.is_active()), "{status:?}");
        }
    }
}
