// ============================================================================
// Celestial Plugin - Day/Night Cycle and Atmosphere Integration
// ============================================================================
//
// This plugin derives the *readable* state of the day/night cycle:
// - CelestialState: sun/moon direction, elevation, colour, star visibility
// - TimeOfDayCurve: smooth curves other systems interpolate against
// - Atmosphere colour transitions across sunrise, noon and sunset
//
// ## What this plugin deliberately does NOT do
//
// It does not write GlobalAmbientLight and it does not write any
// DirectionalLight. It used to do both, and both were bugs:
//
// - `update_ambient_lighting` wrote GlobalAmbientLight with a brightness of
//   `0.3 + 0.4 * elevation/90`, while SharedLightingPlugin wrote the same
//   resource with `brightness * 500 * night_factor`. Neither system was ordered
//   against the other, so the scene's ambient level depended on schedule order.
// - `sync_directional_light_with_sun` queried `Without<Sun>`, which matches
//   every directional light that is not the sun entity — including the Moon —
//   and drove them all with *sun* data, overwriting the moon's own orbital
//   mechanics every frame.
//
// GlobalAmbientLight is owned by `SharedLightingPlugin::update_ambient_light`,
// the sun light by `update_sun_position`, the moon light by
// `update_moon_position`. One writer each.
//
// It also no longer integrates its own clock. LightingService is the single
// authority for time of day; a Sun authored with `cycle_speed` feeds that
// service cycle instead of running a second integrator against it.
//
// NOTE: Sun/Moon visuals are rendered by Bevy's atmosphere via the SunDisk
// component on DirectionalLight (see sky_atmosphere.rs). No billboard meshes.
//
// Table of Contents:
// 1. Plugin Definition
// 2. Resources (CelestialState, TimeOfDayCurve)
// 3. Systems (update_celestial_cycle, sync_sun_cycle_to_service, sync_atmosphere_with_time)
// 4. Helper Functions
// ============================================================================

use bevy::prelude::*;
use eustress_common::classes::{Sun, Moon, Sky, Atmosphere};
use eustress_common::services::lighting::LightingService;

// ============================================================================
// 1. Plugin Definition
// ============================================================================

/// Plugin for celestial body management and day/night cycle
pub struct CelestialPlugin;

impl Plugin for CelestialPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CelestialState>()
            .init_resource::<TimeOfDayCurve>()
            .add_systems(Update, (
                sync_sun_cycle_to_service,
                update_celestial_cycle,
                update_star_visibility,
                sync_atmosphere_with_time_of_day,
            ).chain());
    }
}

// ============================================================================
// 2. Resources
// ============================================================================

/// Time-of-day curve resource for AAA lighting transitions
/// Provides smooth interpolation values for atmosphere, lighting, and colors
#[derive(Resource)]
pub struct TimeOfDayCurve {
    /// Current normalized time (0.0 = midnight, 0.25 = 6am, 0.5 = noon, 0.75 = 6pm)
    pub normalized_time: f32,
    /// Sun intensity multiplier (0.0 at night, 1.0 at noon)
    pub sun_intensity_curve: f32,
    /// Atmosphere density multiplier based on time
    pub atmosphere_density_curve: f32,
    /// Sky color blend factor (0.0 = night, 1.0 = day)
    pub sky_blend: f32,
    /// Horizon glow intensity (peaks at sunrise/sunset)
    pub horizon_glow: f32,
    /// Ambient light multiplier
    pub ambient_multiplier: f32,
    /// Current sky tint color (interpolated)
    pub sky_tint: [f32; 4],
    /// Current horizon color (interpolated)
    pub horizon_color: [f32; 4],
}

impl Default for TimeOfDayCurve {
    fn default() -> Self {
        Self {
            normalized_time: 0.5, // Noon
            sun_intensity_curve: 1.0,
            atmosphere_density_curve: 0.3,
            sky_blend: 1.0,
            horizon_glow: 0.0,
            ambient_multiplier: 1.0,
            sky_tint: [0.5, 0.7, 1.0, 1.0], // Clear blue
            horizon_color: [0.9, 0.85, 0.8, 1.0],
        }
    }
}

// ============================================================================
// 3. Resources
// ============================================================================

/// Global celestial state tracking
#[derive(Resource, Default)]
pub struct CelestialState {
    /// Current sun direction (normalized)
    pub sun_direction: Vec3,
    /// Current moon direction (normalized)
    pub moon_direction: Vec3,
    /// Current sun elevation (-90 to 90)
    pub sun_elevation: f32,
    /// Current sun color
    pub sun_color: Color,
    /// Current sun intensity
    pub sun_intensity: f32,
    /// Current moon illumination (0-1)
    pub moon_illumination: f32,
    /// Whether it's currently day
    pub is_day: bool,
    /// Star visibility (0 = hidden, 1 = fully visible)
    pub star_visibility: f32,
    /// Ambient light color
    pub ambient_color: Color,
}

// ============================================================================
// 3. Systems
// ============================================================================

/// Translate an authored `Sun.cycle_speed` into the service-level day cycle.
///
/// `Sun` carries `cycle_speed` (hours of game time per real second) and
/// `cycle_paused`, and `LightingService` carries `cycle_enabled` and
/// `day_length_minutes`. Both used to integrate time independently: the sun
/// advanced `Sun.time_of_day` here while `update_sun_position` advanced
/// `LightingService.time_of_day`, and then `sync_clock_time_to_sun` wrote the
/// service's (differently advanced) clock straight back over the sun's.
///
/// One integrator now, in `SharedLightingPlugin`. This system only converts the
/// authored rate into the equivalent day length, so the `Sun` properties keep
/// meaning exactly what they say.
fn sync_sun_cycle_to_service(
    mut lighting: ResMut<LightingService>,
    sun_query: Query<&Sun, Changed<Sun>>,
) {
    let Some(sun) = sun_query.iter().find(|s| s.enabled) else {
        return;
    };

    let running = !sun.cycle_paused && sun.cycle_speed > 0.0;
    if lighting.cycle_enabled != running {
        lighting.cycle_enabled = running;
    }
    if running {
        // cycle_speed is game-hours per real second, so a full 24 h day takes
        // 24 / cycle_speed real seconds.
        let day_length_minutes = (24.0 / sun.cycle_speed) / 60.0;
        if (lighting.day_length_minutes - day_length_minutes).abs() > 1e-3 {
            lighting.day_length_minutes = day_length_minutes.max(1e-3);
        }
    }
}

/// Derive `CelestialState` and `TimeOfDayCurve` from the current sun and moon.
///
/// Read-only with respect to `Sun`. Mutating `Sun` every frame marked it
/// `Changed`, which made `write_instance_changes_system` flush
/// `Sun.instance.toml` to disk every frame and trip the file watcher, the same
/// stutter loop documented on `update_sun_position`.
fn update_celestial_cycle(
    mut celestial_state: ResMut<CelestialState>,
    mut time_curve: ResMut<TimeOfDayCurve>,
    sun_query: Query<&Sun>,
    moon_query: Query<&Moon>,
) {
    // Find the active sun
    for sun in sun_query.iter() {
        if !sun.enabled {
            continue;
        }

        // Update celestial state from sun
        celestial_state.sun_direction = sun.direction();
        celestial_state.sun_elevation = sun.elevation();
        celestial_state.is_day = sun.is_day();
        
        // Calculate sun color and intensity
        let color = sun.current_color();
        celestial_state.sun_color = Color::srgba(color[0], color[1], color[2], color[3]);
        celestial_state.sun_intensity = sun.current_intensity();
        
        // Calculate ambient color based on time of day
        let day_factor = ((celestial_state.sun_elevation + 10.0) / 30.0).clamp(0.0, 1.0);
        celestial_state.ambient_color = Color::srgba(
            sun.ambient_night_color[0] + (sun.ambient_day_color[0] - sun.ambient_night_color[0]) * day_factor,
            sun.ambient_night_color[1] + (sun.ambient_day_color[1] - sun.ambient_night_color[1]) * day_factor,
            sun.ambient_night_color[2] + (sun.ambient_day_color[2] - sun.ambient_night_color[2]) * day_factor,
            1.0,
        );
        
        // Calculate star visibility (fade in during twilight, full at night)
        celestial_state.star_visibility = if celestial_state.sun_elevation > 0.0 {
            0.0
        } else if celestial_state.sun_elevation > -12.0 {
            // Twilight - fade in stars
            (-celestial_state.sun_elevation / 12.0).clamp(0.0, 1.0)
        } else {
            1.0
        };
        
        // ════════════════════════════════════════════════════════════════
        // Update TimeOfDayCurve for AAA lighting
        // ════════════════════════════════════════════════════════════════
        time_curve.normalized_time = sun.time_of_day / 24.0;
        
        // Calculate smooth curves based on time of day
        // 6am (0.25) = sunrise, 12pm (0.5) = noon, 6pm (0.75) = sunset
        let tod = sun.time_of_day;
        
        // Sun intensity curve: peaks at noon, zero at night
        // Uses smooth sine curve for natural transition
        time_curve.sun_intensity_curve = if tod >= 5.0 && tod <= 19.0 {
            // Daytime: smooth curve peaking at noon
            let day_progress = (tod - 5.0) / 14.0; // 0 at 5am, 1 at 7pm
            (day_progress * std::f32::consts::PI).sin()
        } else {
            0.0
        };
        
        // Horizon glow: peaks at sunrise (6am) and sunset (6pm)
        let sunrise_dist = (tod - 6.0).abs();
        let sunset_dist = (tod - 18.0).abs();
        let min_dist = sunrise_dist.min(sunset_dist);
        time_curve.horizon_glow = (1.0 - min_dist / 2.0).clamp(0.0, 1.0).powf(2.0);
        
        // Sky blend: 0 = night colors, 1 = day colors
        time_curve.sky_blend = if tod >= 5.0 && tod <= 7.0 {
            // Sunrise transition
            (tod - 5.0) / 2.0
        } else if tod >= 17.0 && tod <= 19.0 {
            // Sunset transition
            1.0 - (tod - 17.0) / 2.0
        } else if tod > 7.0 && tod < 17.0 {
            1.0 // Full day
        } else {
            0.0 // Night
        };
        
        // Atmosphere density: slightly higher at sunrise/sunset for warm glow
        time_curve.atmosphere_density_curve = 0.3 + time_curve.horizon_glow * 0.3;
        
        // Ambient multiplier
        time_curve.ambient_multiplier = 0.1 + time_curve.sky_blend * 0.9;
        
        // Calculate sky tint color based on time of day
        // Night: dark blue, Sunrise/Sunset: orange/pink, Day: light blue
        if time_curve.horizon_glow > 0.3 {
            // Sunrise/Sunset colors
            let glow = time_curve.horizon_glow;
            time_curve.sky_tint = [
                0.5 + glow * 0.5,  // More red
                0.4 + glow * 0.2,  // Some green
                0.6 - glow * 0.3,  // Less blue
                1.0,
            ];
            time_curve.horizon_color = [
                1.0,               // Full red
                0.4 + glow * 0.2,  // Orange
                0.2,               // Minimal blue
                1.0,
            ];
        } else if time_curve.sky_blend > 0.5 {
            // Daytime: clear blue sky
            time_curve.sky_tint = [0.5, 0.7, 1.0, 1.0];
            time_curve.horizon_color = [0.85, 0.9, 0.95, 1.0];
        } else {
            // Nighttime: dark blue
            time_curve.sky_tint = [0.05, 0.05, 0.15, 1.0];
            time_curve.horizon_color = [0.1, 0.1, 0.2, 1.0];
        }
        
        break; // Only process first enabled sun
    }
    
    // Update moon state
    for moon in moon_query.iter() {
        if !moon.enabled {
            continue;
        }
        
        celestial_state.moon_direction = moon.direction(celestial_state.sun_direction);
        celestial_state.moon_illumination = moon.illumination();
        
        break; // Only process first enabled moon
    }
}

// `sync_directional_light_with_sun` and `update_ambient_lighting` used to live
// here. Both were removed rather than repaired, because both were writing
// resources that already had an owner:
//
// - The light sync's `Without<Sun>` filter matched every directional light in
//   the scene that was not the sun entity. That includes the Moon, so moonlight
//   colour, intensity and direction were overwritten with sun-derived values
//   every frame, silently defeating `update_moon_position`'s orbital mechanics.
//   The sun light is owned by `update_sun_position`; the moon light by
//   `update_moon_position`.
// - The ambient write competed with two systems in SharedLightingPlugin using
//   an incompatible scale (a brightness of ~0.3 against their ~500), with no
//   ordering between them. `GlobalAmbientLight` is now owned solely by
//   `update_ambient_light`, which folds in time of day, exposure compensation
//   and the diffuse environment scale in one place.
//
// `CelestialState` still publishes sun/moon direction, colour and illumination,
// so anything that wants to read those (the clouds plugin does) still can.

/// Update star visibility in Sky components
fn update_star_visibility(
    celestial_state: Res<CelestialState>,
    mut sky_query: Query<&mut Sky>,
) {
    for mut sky in sky_query.iter_mut() {
        // Stars are visible based on celestial state
        // The actual star_count is a property, but we could add a visibility multiplier
        // For now, this system just ensures sky components are aware of night state
        if celestial_state.star_visibility > 0.5 {
            sky.celestial_bodies_shown = true;
        }
    }
}

/// Synchronize Atmosphere properties with time of day for AAA effects
/// This creates realistic sky color transitions at sunrise/sunset
fn sync_atmosphere_with_time_of_day(
    time_curve: Res<TimeOfDayCurve>,
    mut atmosphere_query: Query<&mut Atmosphere>,
) {
    // Only update if time curve has meaningful values
    if !time_curve.is_changed() {
        return;
    }
    
    for mut atmosphere in atmosphere_query.iter_mut() {
        // Blend atmosphere color based on time of day
        // This creates the warm sunrise/sunset and cool night effects
        atmosphere.color = time_curve.sky_tint;
        atmosphere.decay = time_curve.horizon_color;
        
        // Adjust glare based on horizon glow (sun near horizon = more glare)
        atmosphere.glare = time_curve.horizon_glow * 0.5;
        
        // Haze increases slightly at sunrise/sunset for atmospheric effect
        // but keep base haze from user settings
        let base_haze = atmosphere.haze;
        let time_haze = time_curve.horizon_glow * 0.15;
        atmosphere.haze = (base_haze + time_haze).min(1.0);
    }
}

// ============================================================================
// 4. Helper Functions
// ============================================================================

/// Set time of day on all Sun components
pub fn set_time_of_day(sun_query: &mut Query<&mut Sun>, time: f32) {
    for mut sun in sun_query.iter_mut() {
        sun.time_of_day = time.clamp(0.0, 24.0);
    }
}

/// Get formatted time string (HH:MM)
pub fn format_time(time_of_day: f32) -> String {
    let hours = time_of_day.floor() as u32;
    let minutes = ((time_of_day - hours as f32) * 60.0).floor() as u32;
    format!("{:02}:{:02}", hours, minutes)
}

/// Get time period name
pub fn get_time_period(sun_elevation: f32) -> &'static str {
    match sun_elevation {
        e if e > 30.0 => "Day",
        e if e > 0.0 => "Morning/Evening",
        e if e > -6.0 => "Civil Twilight",
        e if e > -12.0 => "Nautical Twilight",
        e if e > -18.0 => "Astronomical Twilight",
        _ => "Night",
    }
}
