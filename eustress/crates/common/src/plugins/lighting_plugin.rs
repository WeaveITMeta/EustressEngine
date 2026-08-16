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
use crate::plugins::reflections::ReflectionsPlugin;
use crate::plugins::sky_atmosphere::SkyAtmospherePlugin;
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
    SkyCamera, SkyConfig, SkyMode, StarField,
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
                    sync_clock_time_to_sun,
                    update_sun_position.after(sync_clock_time_to_sun),
                    update_moon_position.after(sync_clock_time_to_sun),
                    // Sole owner of GlobalAmbientLight. Ordered after the sun so
                    // it reads this frame's elevation, not last frame's.
                    update_ambient_light.after(update_sun_position),
                    update_fog_settings,
                    sync_sun_class_to_sundisk,
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
/// `SunClass::current_intensity()` there); the two are merged, with `SunClass`
/// winning because its noon/horizon interpolation is the better model and it is
/// what the Properties panel edits.
fn update_sun_position(
    lighting: Option<ResMut<LightingService>>,
    mut sun_query: Query<(&mut DirectionalLight, &mut Transform), With<SunMarker>>,
    sun_class_query: Query<&SunClass, With<SunMarker>>,
    time: Res<Time>,
) {
    let Some(mut lighting) = lighting else { return };

    if lighting.cycle_enabled {
        let day_length_secs = lighting.day_length_minutes * 60.0;
        if day_length_secs > 0.0 {
            lighting.time_of_day += time.delta_secs() / day_length_secs;
            if lighting.time_of_day > 1.0 {
                lighting.time_of_day -= 1.0;
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
    let sun_class_changed = !sun_class_query.is_empty() && lighting.is_changed();
    if !lighting.is_changed() && !lighting.cycle_enabled && !sun_class_changed {
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
    match sun_class {
        // SunClass models colour and intensity against solar elevation, so a
        // low sun reddens and dims the way it should.
        Some(sc) => {
            sun_light.color = arr_to_color(sc.current_color());
            sun_light.illuminance = sc.current_intensity() * scale;
            sun_light.shadow_maps_enabled = sc.cast_shadows && sun_dir.y > 0.05;
        }
        None => {
            sun_light.color = arr_to_color(lighting.sun_color);
            sun_light.illuminance = lighting.sun_intensity * sun_dir.y.max(0.0).powf(0.4) * scale;
            sun_light.shadow_maps_enabled = lighting.shadows_enabled && sun_dir.y > 0.05;
        }
    }

    sun_transform.translation = sun_dir * 100.0;
    sun_transform.look_at(Vec3::ZERO, Vec3::Y);
}

/// Keep `SunDisk::angular_size` in step with the authored `Sun.angular_size`.
///
/// The disc is drawn by bevy's atmosphere from this component, so it is the one
/// place the sun's apparent size is set.
fn sync_sun_class_to_sundisk(mut sun_query: Query<(&SunClass, &mut SunDisk), Changed<SunClass>>) {
    for (sun_class, mut sun_disk) in sun_query.iter_mut() {
        let new_angular_size = sun_class.angular_size.to_radians();
        if (sun_disk.angular_size - new_angular_size).abs() > 0.001 {
            sun_disk.angular_size = new_angular_size;
            info!(
                "☀️ Sun angular_size synced: {:.1}° → {:.4} rad",
                sun_class.angular_size, new_angular_size
            );
        }
    }
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
        if (sun.time_of_day - time_of_day).abs() > 0.01 {
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

/// Drive the moon's `DirectionalLight` from real orbital mechanics.
///
/// The sole owner of the moon light. The engine's celestial plugin used to
/// reach it too, through a `Without<Sun>` query that matched every directional
/// light that was not the sun, and drove it with *sun* data.
fn update_moon_position(
    lighting: Res<LightingService>,
    mut moon_query: Query<(&mut DirectionalLight, &mut Transform, &MoonClass), With<MoonMarker>>,
    sun_query: Query<&SunClass, With<SunMarker>>,
) {
    if !lighting.is_changed() && !lighting.cycle_enabled {
        return;
    }

    let sun_data = sun_query.iter().next().cloned().unwrap_or_else(|| SunClass {
        time_of_day: lighting.time_of_day * 24.0,
        latitude: lighting.geographic_latitude,
        ..Default::default()
    });

    if let Ok((mut moon_light, mut moon_transform, moon_data)) = moon_query.single_mut() {
        let moon_dir = moon_data.direction_realistic(&sun_data);
        let sun_elevation = sun_data.elevation();
        let phase_illumination = moon_data.illumination();

        moon_light.color = arr_to_color(moon_data.color);
        moon_light.illuminance =
            (moon_data.current_intensity(sun_elevation) * phase_illumination).max(0.01);
        moon_light.shadow_maps_enabled =
            moon_data.cast_shadows && sun_elevation < -0.1 && phase_illumination > 0.3;

        moon_transform.translation = moon_dir * 100.0;
        moon_transform.look_at(Vec3::ZERO, Vec3::Y);
    }
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
/// It scales the **sun**, not just the ambient fill. Previously it multiplied
/// only ambient, which — once image-based lighting became the primary ambient
/// source and the fill dropped to a fraction of its old value — left the slider
/// with almost no visible authority. Sunlight is what a brightness control is
/// expected to move.
fn brightness_scale(lighting: &LightingService) -> f32 {
    (lighting.brightness / BRIGHTNESS_REFERENCE).clamp(0.0, 64.0)
}

/// Ambient fill when image-based lighting is carrying the scene.
///
/// With a working environment map the sky is the primary source of ambient
/// light, so this term is a small fill on top rather than the main event.
const AMBIENT_FILL_BASE: f32 = 80.0;

/// Ambient when there is no environment map to lean on.
///
/// If the author disables the environment map (or the GPU cannot run bevy's
/// filtering compute pipelines), ambient has to carry the scene alone or
/// everything not directly lit goes black.
const AMBIENT_NO_IBL_BASE: f32 = 500.0;

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
    sun_class_query: Query<&SunClass, With<SunMarker>>,
    mut ambient: ResMut<GlobalAmbientLight>,
) {
    let sun_dir = sun_class_query
        .iter()
        .next()
        .map(|sc| sc.direction())
        .unwrap_or_else(|| lighting.sun_direction());
    let sun_y = sun_dir.y;

    // 1.0 in daylight, easing to 0.02 once the sun is well below the horizon.
    let night_factor = if sun_y > 0.1 {
        1.0
    } else if sun_y > -0.15 {
        ((sun_y + 0.15) / 0.25).clamp(0.02, 1.0)
    } else {
        0.02
    };

    let ibl_active = scene_atmosphere.atmosphere.environment_map_enabled
        && scene_atmosphere.atmosphere.environment_intensity > 0.0;
    let base = if ibl_active { AMBIENT_FILL_BASE } else { AMBIENT_NO_IBL_BASE };

    ambient.color = arr_to_color(lighting.ambient);
    ambient.brightness = base
        * brightness_scale(&lighting)
        * night_factor
        * lighting.environment_diffuse_scale.max(0.0);
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
    fn ambient_drops_to_a_fill_once_ibl_is_carrying_the_scene() {
        // The regression this guards: before the environment map worked, ambient
        // was 500x and doing all the work. With real IBL that would double-light
        // everything.
        assert!(AMBIENT_FILL_BASE < AMBIENT_NO_IBL_BASE);
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
