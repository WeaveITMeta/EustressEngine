//! # Shared Lighting Plugin
//!
//! Common lighting for both Engine and Client: the sun, the moon, ambient fill,
//! and global distance fog.
//!
//! Sky rendering, atmospheric scattering and image-based lighting live in
//! [`crate::plugins::sky_atmosphere`]; local reflections live in
//! [`crate::plugins::reflections`]. This plugin pulls both in, so adding
//! `SharedLightingPlugin` still gets you the whole lighting stack.
//!
//! ## One writer per resource
//!
//! Every value here has exactly one system that owns it. That is not a style
//! preference: three unordered systems used to write `GlobalAmbientLight` with
//! scales that differed by three orders of magnitude (`brightness * 500 *
//! night`, `brightness * 500 * exposure`, and `0.3 + 0.4 * elevation`), so which
//! one you got depended on schedule order. Two more fought over the sun's
//! `DirectionalLight` with different intensity curves.
//!
//! - `GlobalAmbientLight` is owned by [`update_ambient_light`] and nothing else.
//!   Time of day, exposure compensation and the diffuse environment scale are
//!   all folded into that one write.
//! - The sun's `DirectionalLight` is owned by [`update_sun_position`].
//! - The moon's `DirectionalLight` is owned by [`update_moon_position`].

use bevy::prelude::*;
use bevy::light::{GlobalAmbientLight, SunDisk};
use bevy::pbr::{DistanceFog, FogFalloff};
use tracing::info;

use crate::classes::{Moon as MoonClass, Sky, Sun as SunClass};
use crate::plugins::moon_disc::MoonDiscPlugin;
use crate::plugins::reflections::ReflectionsPlugin;
use crate::plugins::sky_atmosphere::{
    luminance, moonlight_color, moonlight_lux, SkyAtmospherePlugin, SkyExposure, SkyLight,
    SkyLightSet,
};
use crate::plugins::volumetric_clouds::VolumetricCloudsPlugin;
use crate::services::lighting::{
    EustressAtmosphere, FillLight, LightingService, Moon as MoonMarker, Sun as SunMarker,
};

// Re-exported so existing call sites keep working now that these live in the
// sky module. `NoAtmosphere` in particular is used by the engine's AI camera and
// the Slint overlay camera to opt out of sky handling entirely.
// `SkyConfig`, not `SkySettings`: `services::lighting` already owns that name
// for a different thing (procedural/sun-visible/star-visible toggles).
pub use crate::plugins::sky_atmosphere::{
    create_gradient_skybox, create_star_field, ActiveSkyMode, NoAtmosphere, SceneAtmosphere,
    SkyBillboard, SkyCamera, SkyConfig, SkyMode, StarField,
};

// ============================================================================
// Plugin
// ============================================================================

/// Shared lighting plugin for Engine and Client.
pub struct SharedLightingPlugin;

impl Plugin for SharedLightingPlugin {
    fn build(&self, app: &mut App) {
        app
            // Sky, atmosphere and image-based lighting.
            .add_plugins(SkyAtmospherePlugin)
            // Local reflection probes and (opt-in) screen-space reflections.
            .add_plugins(ReflectionsPlugin)
            // The moon's disc and the volumetric cloud layer, both drawn at
            // sky distance and lit from `SkyLight`.
            .add_plugins((MoonDiscPlugin, VolumetricCloudsPlugin))
            .init_resource::<LightingService>()
            .register_type::<LightingService>()
            .register_type::<Sky>()
            .register_type::<SunMarker>()
            .register_type::<SunClass>()
            .register_type::<MoonClass>()
            .register_type::<FillLight>()
            .add_systems(Startup, setup_lighting)
            .add_systems(
                Update,
                (
                    // The clock drives the classes the sky light is computed
                    // from, so it runs first.
                    sync_clock_time_to_sun.before(SkyLightSet),
                    update_sun_position.after(sync_clock_time_to_sun).before(SkyLightSet),
                    update_moon_position.after(SkyLightSet),
                    // Sole owner of GlobalAmbientLight. After the sky light, so
                    // it reads this frame's sun and moon, not last frame's.
                    update_ambient_light.after(SkyLightSet),
                    update_fog_settings,
                    sync_sun_disk.after(SkyLightSet),
                    hide_moon_sun_disk,
                ),
            );
    }
}

// ============================================================================
// Setup
// ============================================================================

fn arr_to_color(arr: [f32; 4]) -> Color {
    Color::srgba(arr[0], arr[1], arr[2], arr[3])
}

/// Set the ambient baseline. Everything else is loaded per Space.
///
/// Sun and Moon entities are deliberately not spawned here: each Space owns its
/// lighting through `Lighting/*.instance.toml`. The file loader spawns bare
/// `Instance` entities and the engine's `hydrate_lighting_entities` attaches
/// `DirectionalLight`, `SunMarker`, cascade shadows and `SunDisk` on the next
/// frame. That keeps a Space switch from leaving duplicates behind, and lets
/// each Space carry its own time of day and latitude.
fn setup_lighting(mut commands: Commands) {
    commands.insert_resource(GlobalAmbientLight::NONE);
    info!("💡 SharedLightingPlugin ready (lighting entities load from the Space)");
}

// ============================================================================
// Sun
// ============================================================================

/// Advance the day/night cycle and drive the sun's `DirectionalLight`.
///
/// The single owner of the sun light. The engine used to run a second system
/// over the same entity with a different intensity curve
/// (`lighting.sun_intensity * elevation^0.4` here against
/// `SunClass::current_intensity()` there); the two are merged.
///
/// ## The atmosphere colours the sun; this must not
///
/// On the atmosphere path bevy multiplies every directional light, on every
/// surface and in the volumetric fog, by the atmosphere's transmittance
/// toward it and by how much of its disc clears the horizon. That IS the
/// low sun's reddening and dimming, computed from the same medium that draws
/// the sky. So the light carries the sun as it is above the atmosphere:
/// `SunClass::current_color()` and `current_intensity()` reddened and dimmed
/// it a second time, which is why golden hour came out a dim brick red
/// (a 5 degree sun at about 3,000 lux instead of 35,000), and their twilight
/// ramp cut the light that lights the twilight sky to zero at 6 degrees down,
/// switching the blue hour off. Those curves remain for the cubemap paths,
/// which have no atmosphere to do the work.
fn update_sun_position(
    lighting: Option<ResMut<LightingService>>,
    active: Option<Res<ActiveSkyMode>>,
    mut sun_query: Query<(&mut DirectionalLight, &mut Transform), With<SunMarker>>,
    sun_class_query: Query<&SunClass, With<SunMarker>>,
    // The Sun appears LONG after startup: the file loader spawns it when the
    // Space finishes loading, and the engine hydrates `SunClass` onto it a frame
    // later. Both happen well after `LightingService`'s initial change tick, so
    // gating only on `lighting.is_changed()` meant this system had already
    // stopped running by the time there was a sun to drive. The light kept
    // whatever illuminance the loader gave it and `sun_intensity` reached
    // nothing — measured: raising it from 15,000 to 130,000 lux changed not one
    // pixel of the scene.
    sun_dirty: Query<(), (With<SunMarker>, Or<(Added<SunMarker>, Changed<SunClass>)>)>,
    time: Res<Time>,
    mut last_reported: Local<f32>,
) {
    let Some(mut lighting) = lighting else { return };

    if lighting.cycle_enabled {
        let day_length_secs = lighting.day_length_minutes * 60.0;
        if day_length_secs > 0.0 {
            lighting.time_of_day += time.delta_secs() / day_length_secs;
            if lighting.time_of_day > 1.0 {
                lighting.time_of_day -= 1.0;
            }
            // Keep the clock in step. `sync_clock_time_to_sun` and the
            // Properties panel both read the clock string, so advancing only
            // `time_of_day` left the sun where the clock said, frozen, for as
            // long as the cycle ran.
            let clock = format_clock(lighting.time_of_day);
            if lighting.clock_time != clock {
                lighting.clock_time = clock;
            }
        }
    } else {
        lighting.bypass_change_detection();
    }

    // Only touch the Sun entity when something actually changed. Without this
    // guard every frame mutably borrows `sun_transform`, which marks
    // `Changed<Transform>` and makes `write_instance_changes_system` flush
    // `Sun.instance.toml` to disk every frame, which trips the file watcher,
    // which re-runs the class-schema self-heal every couple of seconds. The
    // symptom is a visible FPS stutter with no obvious cause.
    //
    // `sun_dirty` is what makes the guard correct rather than merely cheap: the
    // sun arriving is itself a change, and it arrives after the resource has
    // gone quiet.
    let mode_changed = active.as_ref().is_some_and(|mode| mode.is_changed());
    if !lighting.is_changed() && !lighting.cycle_enabled && sun_dirty.is_empty() && !mode_changed {
        return;
    }

    let Ok((mut sun_light, mut sun_transform)) = sun_query.single_mut() else {
        return;
    };

    let sun_class = sun_class_query.iter().next();
    let sun_dir = sun_class
        .map(|sc| sc.direction())
        .unwrap_or_else(|| lighting.sun_direction());

    let scale = brightness_scale(&lighting);
    let atmosphere = active.is_some_and(|mode| mode.0 == SkyMode::Atmosphere);
    // Shadows stay on until the whole disc is down (it is 0.53 degrees
    // across). The atmosphere lights surfaces from a grazing sun, and a
    // light with no shadow map shines straight through walls: the old
    // 3 degree cut-off lit every interior at sunrise and sunset.
    let sun_up = sun_dir.y > -0.005;
    // The atmosphere keeps a sun below the horizon off every surface, but it
    // is that sun that lights the twilight sky, so the light stays on through
    // dusk. By 10 degrees down that sky is black at any exposure; past it the
    // light is faded out, so a camera that renders without the atmosphere
    // (the AI capture camera) is not lit from under the ground all night.
    let twilight = ((sun_dir.y.clamp(-1.0, 1.0).asin().to_degrees() + 16.0) / 6.0).clamp(0.0, 1.0);
    match sun_class {
        Some(sc) if atmosphere => {
            sun_light.color = arr_to_color(sc.noon_color);
            sun_light.illuminance = sc.noon_intensity.max(0.0) * scale * twilight;
            sun_light.shadow_maps_enabled = sc.cast_shadows && sun_up;
        }
        // No atmosphere to redden a low sun: `SunClass` models colour and
        // intensity against solar elevation instead.
        Some(sc) => {
            sun_light.color = arr_to_color(sc.current_color());
            sun_light.illuminance = sc.current_intensity() * scale;
            sun_light.shadow_maps_enabled = sc.cast_shadows && sun_dir.y > 0.05;
        }
        None if atmosphere => {
            sun_light.color = arr_to_color(lighting.sun_color);
            sun_light.illuminance = lighting.sun_intensity.max(0.0) * scale * twilight;
            sun_light.shadow_maps_enabled = lighting.shadows_enabled && sun_up;
        }
        None => {
            sun_light.color = arr_to_color(lighting.sun_color);
            sun_light.illuminance = lighting.sun_intensity * sun_dir.y.max(0.0).powf(0.4) * scale;
            sun_light.shadow_maps_enabled = lighting.shadows_enabled && sun_dir.y > 0.05;
        }
    }

    sun_transform.translation = sun_dir * 100.0;
    sun_transform.look_at(Vec3::ZERO, Vec3::Y);

    // Report the sun's resolved illuminance whenever it moves materially. The
    // scene's exposure is calibrated in physical lux, so this number is the one
    // that decides whether the render is correctly lit; when it silently kept
    // the loader's placeholder there was nothing to read that said so.
    let lux = sun_light.illuminance;
    if (lux - *last_reported).abs() > (*last_reported * 0.05).max(1.0) {
        *last_reported = lux;
        info!("☀️ Sun illuminance {lux:.0} lux (elevation {:.1}°)", sun_dir.y.asin().to_degrees());
    }
}

/// The brightest the sun's disc is drawn, as a pre-tonemap value.
///
/// Bevy draws the disc energy-conserving: its radiance is the light's
/// illuminance over the disc's solid angle. For the true 0.53 degree sun at
/// `RAW_SUNLIGHT` and ev100 13 that is about 175,000, and the HDR target is
/// 16-bit float, which ends at 65,504; past it the value becomes infinity
/// and bloom smears it into a block. Anything above a few hundred is already
/// pure white on screen, so the cap costs no brightness the eye could see and
/// still leaves bloom a disc far brighter than the sky to glare from.
pub const SUN_DISC_PEAK: f32 = 16_000.0;

/// `SunDisk::intensity` that draws a disc of `angular_size` radians for a
/// light of `illuminance` lux no brighter than [`SUN_DISC_PEAK`] at `ev100`.
pub fn sun_disk_intensity(illuminance: f32, angular_size: f32, ev100: f32) -> f32 {
    let solid_angle = angular_size * angular_size * 0.25 * std::f32::consts::PI;
    let exposure = (-ev100).exp2() / 1.2;
    let peak = illuminance.max(0.0) / solid_angle.max(1e-9) * exposure;
    if peak <= SUN_DISC_PEAK {
        1.0
    } else {
        SUN_DISC_PEAK / peak
    }
}

/// Keep the drawn sun disc sized from `Sun.angular_size` and below the HDR
/// ceiling at the current exposure.
///
/// The disc is drawn by bevy's atmosphere from this component, so it is the one
/// place the sun's apparent size is set.
fn sync_sun_disk(
    exposure: Res<SkyExposure>,
    mut suns: Query<(&SunClass, &DirectionalLight, &mut SunDisk), With<SunMarker>>,
    skies: Query<&Sky>,
) {
    let shown = skies.iter().next().map_or(true, |s| s.celestial_bodies_shown);
    for (sun_class, light, mut disk) in suns.iter_mut() {
        let angular_size = sun_class.angular_size.clamp(0.05, 20.0).to_radians();
        let intensity = if shown {
            sun_disk_intensity(light.illuminance, angular_size, exposure.camera_ev100)
        } else {
            0.0
        };
        if (disk.angular_size - angular_size).abs() > 1e-5 {
            disk.angular_size = angular_size;
        }
        if (disk.intensity - intensity).abs() > disk.intensity.max(intensity) * 0.02 + 1e-6 {
            disk.intensity = intensity;
        }
    }
}

/// Stop bevy drawing a second sun where the moon is.
///
/// Bevy's atmosphere draws a sun disc for EVERY directional light, and a
/// light without a `SunDisk` gets `SunDisk::EARTH`: a flat, sun-bright disc
/// sat exactly where the moon is, a small white hole in front of the moon's
/// own disc. Intensity 0 turns the drawing off; the angular size is kept so
/// the atmosphere still eases moonlight off as the moon sets.
fn hide_moon_sun_disk(
    mut commands: Commands,
    moons: Query<(Entity, &MoonClass, Option<&SunDisk>), With<MoonMarker>>,
) {
    for (entity, moon, disk) in moons.iter() {
        let angular_size = moon.angular_size.clamp(0.05, 20.0).to_radians();
        let wanted = SunDisk { angular_size, intensity: 0.0 };
        let stale = disk.map_or(true, |d| {
            d.intensity != 0.0 || (d.angular_size - angular_size).abs() > 1e-5
        });
        if stale {
            commands.entity(entity).insert(wanted);
        }
    }
}

/// `time_of_day` (0..1) as the `HH:MM:SS` the clock property shows.
fn format_clock(time_of_day: f32) -> String {
    let total = (time_of_day.rem_euclid(1.0) * 86_400.0).round() as u32 % 86_400;
    format!("{:02}:{:02}:{:02}", total / 3600, (total / 60) % 60, total % 60)
}

/// Parse `LightingService.clock_time` into `Sun.time_of_day`.
fn sync_clock_time_to_sun(
    lighting: Res<LightingService>,
    mut sun_query: Query<&mut SunClass, With<SunMarker>>,
) {
    if !lighting.is_changed() {
        return;
    }
    let time_of_day =
        parse_clock_time(&lighting.clock_time).unwrap_or(lighting.time_of_day * 24.0);
    for mut sun in sun_query.iter_mut() {
        // A second of clock time. The old hundredth of an hour (36 s, a
        // sixth of a degree of sun) stepped the running day cycle visibly.
        if (sun.time_of_day - time_of_day).abs() > 1.0 / 3600.0 {
            sun.time_of_day = time_of_day;
        }
    }
}

/// Parse `"HH:MM:SS"`, `"HH:MM"` or `"HH"` into hours.
fn parse_clock_time(clock_time: &str) -> Option<f32> {
    let parts: Vec<&str> = clock_time.split(':').collect();
    let hours: f32 = parts.first()?.trim().parse().ok()?;
    let minutes: f32 = parts.get(1).and_then(|s| s.trim().parse().ok()).unwrap_or(0.0);
    let seconds: f32 = parts.get(2).and_then(|s| s.trim().parse().ok()).unwrap_or(0.0);
    Some(hours + minutes / 60.0 + seconds / 3600.0)
}

// ============================================================================
// Moon
// ============================================================================

/// Drive the moon's `DirectionalLight` from its place on the sky.
///
/// The sole owner of the moon light. The engine's celestial plugin used to
/// reach it too, through a `Without<Sun>` query that matched every directional
/// light that was not the sun, and drove it with *sun* data.
///
/// Gated like the sun, on the moon arriving as well as on the resource. It
/// used to wait for `lighting.is_changed()` alone, but the moon hydrates when
/// its Space loads, long after the resource last changed, so the light kept
/// its hydration placeholder (500 lux, from a fixed point overhead, day and
/// night) until someone happened to edit the clock.
///
/// The light is the moon above the atmosphere, as the sun's is: bevy's
/// atmosphere dims and reddens it on the way down. Its phase follows Allen's
/// law (a quarter moon is a tenth of a full one) and it carries
/// [`crate::plugins::sky_atmosphere::MOONLIGHT_GAIN`].
fn update_moon_position(
    lighting: Res<LightingService>,
    active: Option<Res<ActiveSkyMode>>,
    sky_light: Res<SkyLight>,
    mut moon_query: Query<(&mut DirectionalLight, &mut Transform, &MoonClass), With<MoonMarker>>,
    moon_dirty: Query<(), (With<MoonMarker>, Or<(Added<MoonMarker>, Changed<MoonClass>)>)>,
    sun_dirty: Query<(), (With<SunMarker>, Or<(Added<SunMarker>, Changed<SunClass>)>)>,
) {
    let mode_changed = active.as_ref().is_some_and(|mode| mode.is_changed());
    if !lighting.is_changed()
        && !lighting.cycle_enabled
        && moon_dirty.is_empty()
        && sun_dirty.is_empty()
        && !mode_changed
    {
        return;
    }

    let Ok((mut moon_light, mut moon_transform, moon_data)) = moon_query.single_mut() else {
        return;
    };
    let moon_dir = sky_light.moon_direction;
    let atmosphere = active.is_some_and(|mode| mode.0 == SkyMode::Atmosphere);
    let color = moonlight_color(moon_data);
    let lux = moonlight_lux(moon_data) * brightness_scale(&lighting);

    moon_light.color = Color::linear_rgb(color.x, color.y, color.z);
    moon_light.illuminance = if atmosphere {
        // Faded out once it has set, for the same reason as the sun: a
        // camera without the atmosphere would be lit from under the ground.
        let risen = ((moon_dir.y.clamp(-1.0, 1.0).asin().to_degrees() + 6.0) / 4.0).clamp(0.0, 1.0);
        lux * risen
    } else {
        // No atmosphere to dim a setting moon: the ground moonlight the sky
        // model computed, in this light's colour.
        luminance(sky_light.moon_ground) / luminance(color).max(1e-6)
    };
    // Moon shadows only once the sun's are gone and the moon is up and bright
    // enough to throw one; two shadowed directional lights cost two sets of
    // cascades, and a sliver of crescent casts nothing worth drawing.
    let sun_down = sky_light.sun_direction.y < -0.035;
    moon_light.shadow_maps_enabled = moon_data.cast_shadows
        && moon_data.enabled
        && sun_down
        && moon_dir.y > 0.02
        && lux > 0.5;

    moon_transform.translation = moon_dir * 100.0;
    moon_transform.look_at(Vec3::ZERO, Vec3::Y);
}

// ============================================================================
// Ambient
// ============================================================================

/// The `brightness` value that means "no change".
///
/// 2.0, matching the shipped Lighting `_service.toml` and Roblox's own
/// `Lighting.Brightness` default, so an untouched Space renders at exactly its
/// authored sun intensity.
const BRIGHTNESS_REFERENCE: f32 = 2.0;

/// How much `LightingService.brightness` scales the scene.
///
/// It scales the **sun** and the moon, not just the ambient fill. Previously it
/// multiplied only ambient, which left the slider with almost no visible
/// authority once image-based lighting became the primary ambient source and
/// the fill dropped to a fraction of its old value. Sunlight is what a
/// brightness control is expected to move.
pub(crate) fn brightness_scale(lighting: &LightingService) -> f32 {
    (lighting.brightness / BRIGHTNESS_REFERENCE).clamp(0.0, 64.0)
}

/// Skylight fill as a fraction of the sun, when the environment map is also
/// lighting the scene.
///
/// Ambient has to be a **fraction of the sun**, not a fixed number. Outdoors a
/// shadowed surface still receives skylight, which is why real shadows are dark
/// but not black. A constant cannot hold that relationship: with the sun at
/// 130,000 lux and ambient pinned at 80, shadows crushed to near-black.
/// Anchoring to the sun keeps it true at any time of day, any authored
/// `sun_intensity`, and any Brightness.
///
/// The value is **calibrated against measured output, not derived**. Bevy hands
/// `ambient_color = colour * brightness` to the shader as radiance and then puts
/// it through `EnvBRDFApprox`, which attenuates it several times below what a
/// naive irradiance model predicts. Deriving this from the physical sky/sun
/// ratio gave 0.08 and measured a sunlit-to-shadow ratio of 46x against a target
/// of 6-10x; the measurement corrected it to 0.45 of the light's illuminance.
///
/// The fill now anchors to the sun that reaches the ground (see
/// [`SkyLight::sun_ground`]) rather than to the light above the atmosphere,
/// and a high sun loses about 13% on the way down, so 0.45 becomes 0.52 to
/// keep the calibrated midday balance.
const SKY_FILL_FRACTION: f32 = 0.52;

/// Skylight fill when there is no environment map to lean on.
///
/// If the author disables the environment map (or the GPU cannot run bevy's
/// filtering compute pipelines) this term carries every unlit surface alone, so
/// it takes over the share the sky would have contributed.
const SKY_FILL_FRACTION_NO_IBL: f32 = 0.86;

/// Minimum ambient, so a moonless night is dark but legible, not black.
///
/// Starlight and airglow, lifted to meet the night exposure: the camera opens
/// up to `MAX_NIGHT_ADAPTATION_EV` stops at night, so this reads about seven
/// stops under a daylit scene. The old 40 was set against the daylight
/// exposure and would read as dusk once the exposure adapts.
const NIGHT_FLOOR_LUX: f32 = 6.0;

/// Shadow fill at night leans toward moonlight's blue, as the eye sees it.
const NIGHT_FILL_TINT: [f32; 3] = [0.75, 0.86, 1.18];

/// The one and only writer of `GlobalAmbientLight`.
///
/// Folds in the inputs that used to be spread across three competing systems:
/// time of day and `environment_diffuse_scale` (which was parsed out of the
/// Properties panel into `LightingService` and then read by nothing at all).
///
/// `exposure_compensation` is deliberately NOT applied here. It is a camera
/// property, and [`crate::plugins::sky_atmosphere`] now sets a real
/// `Exposure` on every managed camera from it. Scaling ambient by it as well
/// would apply it twice.
///
/// `environment_specular_scale` is likewise not applied here; it scales the
/// environment map itself. Bevy exposes a single intensity that drives diffuse
/// and specular IBL together, so the split is "specular scales the sky
/// reflection, diffuse scales this fill".
fn update_ambient_light(
    lighting: Res<LightingService>,
    scene_atmosphere: Res<SceneAtmosphere>,
    sky_light: Res<SkyLight>,
    mut ambient: ResMut<GlobalAmbientLight>,
) {
    // Anchor to the light that actually reaches the ground: the sun and moon
    // after the atmosphere and the horizon. The sun's `DirectionalLight` is
    // now the sun above the atmosphere, constant through the day, so reading
    // its illuminance would light a midnight scene like noon. `SkyLight`
    // already carries `brightness`, which is why it is not applied again.
    let direct_lux = luminance(sky_light.sun_ground) + luminance(sky_light.moon_ground);

    let ibl_active = scene_atmosphere.atmosphere.environment_map_enabled
        && scene_atmosphere.atmosphere.environment_intensity > 0.0;

    // Direct light already falls away at dusk, so the fill follows it down
    // without a separate night curve. The floor keeps night legible.
    let fill = sky_fill_lux(direct_lux, ibl_active);

    let base = sky_fill_color(&lighting);
    let night = sky_light.night;
    let tint = |i: usize| base[i] * (1.0 + (NIGHT_FILL_TINT[i] - 1.0) * night);
    let color = Color::srgba(tint(0), tint(1), tint(2), base[3]);
    if ambient.color != color {
        ambient.color = color;
    }
    let brightness = fill * lighting.environment_diffuse_scale.max(0.0);
    if (ambient.brightness - brightness).abs() > brightness * 1e-4 + 1e-4 {
        ambient.brightness = brightness;
    }
}

/// The colour that skylight fills shadows with.
///
/// This is `outdoor_ambient`, not `ambient`. The two are the Roblox pair this
/// service models: `Ambient` is the global/indoor floor and is **black by
/// default**, while `OutdoorAmbient` is the sky-lit outdoor fill and defaults to
/// mid grey. Reading `ambient` meant the fill was multiplied by black, so no
/// amount of brightness could lift a shadow — raising it 130x moved the darkest
/// shadow pixel from 24 to 25 — and `outdoor_ambient` was parsed into
/// `LightingService` and read by nothing at all.
///
/// Falls back to `ambient` when `outdoor_ambient` is black, so a Space that
/// deliberately drives only the indoor term still gets it, and an author who
/// blacks out both genuinely gets no flat ambient.
fn sky_fill_color(lighting: &LightingService) -> [f32; 4] {
    let is_black = |c: &[f32; 4]| c[0] <= 1e-4 && c[1] <= 1e-4 && c[2] <= 1e-4;
    if is_black(&lighting.outdoor_ambient) {
        lighting.ambient
    } else {
        lighting.outdoor_ambient
    }
}

/// The ambient fill for a given sun, exposed for testing.
fn sky_fill_lux(sun_lux: f32, ibl_active: bool) -> f32 {
    let fraction = if ibl_active { SKY_FILL_FRACTION } else { SKY_FILL_FRACTION_NO_IBL };
    (sun_lux * fraction).max(NIGHT_FLOOR_LUX)
}

// ============================================================================
// Fog
// ============================================================================

/// Apply global distance fog to the primary 3D camera.
///
/// Affects every entity: BaseParts, Terrain, Models.
fn update_fog_settings(
    lighting: Res<LightingService>,
    mut camera_query: Query<(Entity, &Camera, Option<&mut DistanceFog>), With<Camera3d>>,
    mut commands: Commands,
) {
    if !lighting.is_changed() {
        return;
    }

    for (entity, camera, fog) in camera_query.iter_mut() {
        // Only the main 3D camera, not the Slint overlay or the AI camera.
        if camera.order != 0 {
            continue;
        }

        if !lighting.fog_enabled {
            if fog.is_some() {
                commands.entity(entity).remove::<DistanceFog>();
                info!("🌫️ Global fog disabled");
            }
            continue;
        }

        let fog_color = arr_to_color(lighting.fog_color);

        // `end` must be strictly greater than `start` for a linear falloff to
        // mean anything. These values arrive from TOML service files, the
        // Properties panel, Rune scripts and the atmosphere sync, and any of
        // those can get the pair backwards. Swap a reversed pair rather than
        // rendering nonsense, and nudge an equal pair so the falloff maths does
        // not divide by zero.
        let (fog_start, fog_end) = if lighting.fog_end > lighting.fog_start {
            (lighting.fog_start, lighting.fog_end)
        } else if lighting.fog_end < lighting.fog_start {
            tracing::warn!(
                "🌫️ Fog range reversed (start: {}, end: {}) — swapping so end > start",
                lighting.fog_start,
                lighting.fog_end
            );
            (lighting.fog_end, lighting.fog_start)
        } else {
            (lighting.fog_start, lighting.fog_start + 1.0)
        };

        let falloff = FogFalloff::Linear { start: fog_start, end: fog_end };

        if let Some(mut existing_fog) = fog {
            existing_fog.color = fog_color;
            existing_fog.falloff = falloff;
        } else {
            commands
                .entity(entity)
                .insert(DistanceFog { color: fog_color, falloff, ..default() });
            info!("🌫️ Global fog enabled (start: {fog_start}, end: {fog_end})");
        }
    }
}

// ============================================================================
// Atmosphere presets
// ============================================================================

impl SceneAtmosphere {
    /// Build a scene atmosphere from an authored [`EustressAtmosphere`].
    pub fn from_atmosphere(atmosphere: EustressAtmosphere) -> Self {
        Self { atmosphere }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_time_parses_every_authored_shape() {
        assert_eq!(parse_clock_time("14:30:00"), Some(14.5));
        assert_eq!(parse_clock_time("14:30"), Some(14.5));
        assert_eq!(parse_clock_time("14"), Some(14.0));
        assert_eq!(parse_clock_time("06:15:36"), Some(6.26));
        assert_eq!(parse_clock_time(""), None);
        assert_eq!(parse_clock_time("not a time"), None);
    }

    #[test]
    fn ambient_carries_more_when_there_is_no_environment_map() {
        assert!(SKY_FILL_FRACTION < SKY_FILL_FRACTION_NO_IBL);
    }

    #[test]
    fn the_fill_is_a_real_share_of_the_sun_not_a_token() {
        // The report this guards: "generally it's too dark". A fixed ambient of
        // 80 beside a 130,000 lux sun is five thousandths of a percent, so every
        // shadow crushed to black.
        //
        // Deliberately NOT asserting a physical sky:sun ratio. Bevy runs ambient
        // through `EnvBRDFApprox`, so the shader-side result is several times
        // below what irradiance alone predicts and the constant is calibrated
        // against measured pixels instead. What is worth pinning is that the
        // fill stays a substantial, sane share of the sun.
        let sun = bevy::light::light_consts::lux::RAW_SUNLIGHT;
        let share = sky_fill_lux(sun, true) / sun;
        assert!(
            (0.1..=1.0).contains(&share),
            "a fill of {share:.3} of the sun is either invisible or brighter than daylight"
        );
    }

    #[test]
    fn ambient_tracks_the_sun_instead_of_being_a_fixed_number() {
        // A constant cannot hold the sun:sky relationship across time of day or
        // an edited sun_intensity — which is exactly how it broke.
        let bright = sky_fill_lux(130_000.0, true);
        let dim = sky_fill_lux(13_000.0, true);
        assert!((bright / dim - 10.0).abs() < 0.01, "fill must scale with the sun");
    }

    #[test]
    fn skylight_fills_with_outdoor_ambient_not_the_black_indoor_one() {
        // The bug this guards: `ambient` is black by default (correctly — it is
        // the indoor term), so multiplying the fill by it zeroed the whole thing
        // and shadows could not be lifted by any brightness value.
        let mut lighting = LightingService::default();
        lighting.ambient = [0.0, 0.0, 0.0, 1.0];
        lighting.outdoor_ambient = [0.5, 0.5, 0.5, 1.0];
        let c = sky_fill_color(&lighting);
        assert_eq!(c, [0.5, 0.5, 0.5, 1.0], "outdoor ambient must light outdoor shadows");
        assert!(c[0] > 0.0, "a black fill colour makes brightness meaningless");
    }

    #[test]
    fn a_black_outdoor_ambient_falls_back_to_the_indoor_term() {
        let mut lighting = LightingService::default();
        lighting.ambient = [0.2, 0.2, 0.25, 1.0];
        lighting.outdoor_ambient = [0.0, 0.0, 0.0, 1.0];
        assert_eq!(sky_fill_color(&lighting), [0.2, 0.2, 0.25, 1.0]);
    }

    #[test]
    fn a_mid_grey_outdoor_ambient_still_lifts_shadows() {
        // The shipped Space pairs a 130,000 lux sun with a 0.5 grey
        // `outdoor_ambient`. Halving the fill via the colour must still leave a
        // meaningful amount of light — this is the product that actually reaches
        // the shader, and it was zero when the colour came from `ambient`.
        let sun = bevy::light::light_consts::lux::RAW_SUNLIGHT;
        let reaching_shader = 0.5 * sky_fill_lux(sun, true);
        assert!(
            reaching_shader > sun * 0.05,
            "{reaching_shader:.0} against a {sun:.0} lux sun is back to crushed shadows"
        );
    }

    #[test]
    fn night_never_goes_completely_black() {
        // Anchoring to the sun would otherwise reach exactly zero at sunset.
        assert_eq!(sky_fill_lux(0.0, true), NIGHT_FLOOR_LUX);
        assert!(sky_fill_lux(0.0, true) > 0.0);
    }

    #[test]
    fn the_shipped_brightness_is_neutral() {
        // An untouched Space must render at exactly its authored sun intensity,
        // so the default `brightness` and `BRIGHTNESS_REFERENCE` have to agree.
        let lighting = LightingService::default();
        assert_eq!(lighting.brightness, BRIGHTNESS_REFERENCE);
        assert!((brightness_scale(&lighting) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn brightness_has_real_authority_over_the_sun() {
        // The report this guards: "changing lighting properties does not seem to
        // have an impact". Brightness used to scale only the ambient fill, which
        // is a small fraction of the light once image-based lighting carries the
        // scene, so the slider looked broken.
        let mut lighting = LightingService::default();
        lighting.brightness = 4.0;
        assert!((brightness_scale(&lighting) - 2.0).abs() < 1e-6, "double brightness = double sun");
        lighting.brightness = 1.0;
        assert!((brightness_scale(&lighting) - 0.5).abs() < 1e-6);
        // Negative or absurd values must not invert or explode the sun.
        lighting.brightness = -5.0;
        assert_eq!(brightness_scale(&lighting), 0.0);
    }

    #[test]
    fn the_sun_is_physical_sunlight() {
        // The scene renders at ev100 13, bevy's calibration for RAW_SUNLIGHT.
        // A sun authored in some other unit scale silently mismatches it: the
        // old 15,000 lux was ~3.1 EV under, which read as "everything is dark".
        let lighting = LightingService::default();
        assert_eq!(lighting.sun_intensity, bevy::light::light_consts::lux::RAW_SUNLIGHT);
        assert!(
            lighting.sun_intensity > bevy::light::light_consts::lux::FULL_DAYLIGHT,
            "direct sun must exceed diffuse full daylight"
        );
    }

    #[test]
    fn the_true_sun_disc_stays_inside_the_hdr_target() {
        // The HDR target is 16-bit float: 65,504 is the largest finite value,
        // and a sun drawn above it becomes infinity. Measured against the
        // brightest case the Studio sees: raw sunlight, the daylight exposure,
        // the true 0.53 degree disc and the brightness slider doubled.
        let disc = 0.53f32.to_radians();
        let sun = bevy::light::light_consts::lux::RAW_SUNLIGHT * 2.0;
        for ev100 in [13.0f32, 11.0, 8.0, 4.0] {
            let intensity = sun_disk_intensity(sun, disc, ev100);
            let solid_angle = disc * disc * 0.25 * std::f32::consts::PI;
            let peak = sun / solid_angle * intensity * (-ev100).exp2() / 1.2;
            assert!(peak <= SUN_DISC_PEAK * 1.001, "ev100 {ev100}: disc drawn at {peak:.0}");
            assert!(peak < 65_504.0, "ev100 {ev100}: disc overflows f16 at {peak:.0}");
        }
    }

    #[test]
    fn a_dim_disc_is_drawn_at_full_physical_brightness() {
        // The cap only ever dims; a disc that fits keeps intensity 1, so a
        // large authored sun or a weak light is drawn as bevy would draw it.
        assert_eq!(sun_disk_intensity(10.0, 0.53f32.to_radians(), 13.0), 1.0);
        assert_eq!(sun_disk_intensity(130_000.0, 20f32.to_radians(), 13.0), 1.0);
    }

    #[test]
    fn the_running_clock_reads_as_hours_minutes_seconds() {
        assert_eq!(format_clock(0.5), "12:00:00");
        assert_eq!(format_clock(0.25 + 1.0 / 86_400.0 * 30.0), "06:00:30");
        // Wraps rather than reading 24:00:00 or going negative.
        assert_eq!(format_clock(1.0), "00:00:00");
        assert_eq!(format_clock(-0.25), "18:00:00");
        // And it round-trips through the parser the sun reads it with.
        let t = 0.73f32;
        let hours = parse_clock_time(&format_clock(t)).unwrap();
        assert!((hours / 24.0 - t).abs() < 1.0 / 86_400.0 * 1.5);
    }

    #[test]
    fn fog_range_normalisation_handles_a_reversed_pair() {
        // The user-reported case was start: 5000, end: 2000.
        let (start, end) = (5000.0f32, 2000.0f32);
        let (s, e) = if end > start {
            (start, end)
        } else if end < start {
            (end, start)
        } else {
            (start, start + 1.0)
        };
        assert!(e > s, "a reversed pair must come out ordered");
        assert_eq!((s, e), (2000.0, 5000.0));
    }

    #[test]
    fn fog_range_normalisation_handles_an_equal_pair() {
        let (start, end) = (1000.0f32, 1000.0f32);
        let (s, e) = if end > start {
            (start, end)
        } else if end < start {
            (end, start)
        } else {
            (start, start + 1.0)
        };
        assert!(e > s, "an equal pair must not divide by zero");
    }
}
