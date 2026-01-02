// ============================================================================
// Celestial Plugin - Day/Night Cycle and Atmosphere Integration
// ============================================================================
//
// This plugin manages celestial bodies and the day/night cycle:
// - Sun position and lighting based on time of day (6am sunrise, 6pm sunset)
// - Moon phases and night lighting
// - Star rendering at night
// - Directional light synchronization with celestial bodies
// - Atmosphere synchronization with time of day curves
// - AAA lighting with smooth transitions
//
// NOTE: Sun/Moon visuals are rendered by Bevy's Atmosphere shader via SunDisk
// component on DirectionalLight (see lighting_plugin.rs). No billboard meshes.
//
// Table of Contents:
// 1. Plugin Definition
// 2. Resources (CelestialState, TimeOfDayCurve)
// 3. Systems (update_celestial_cycle, sync_directional_light, sync_atmosphere_with_time)
// 4. Helper Functions
// ============================================================================

use bevy::prelude::*;
use eustress_common::classes::{Sun, Moon, Sky, Atmosphere};

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
                update_celestial_cycle,
                sync_directional_light_with_sun,
                update_ambient_lighting,
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

/// Update the celestial cycle based on Sun component settings
/// Also updates the TimeOfDayCurve for AAA lighting transitions
fn update_celestial_cycle(
    time: Res<Time>,
    mut celestial_state: ResMut<CelestialState>,
    mut time_curve: ResMut<TimeOfDayCurve>,
    mut sun_query: Query<&mut Sun>,
    moon_query: Query<&Moon>,
) {
    // Find the active sun
    for mut sun in sun_query.iter_mut() {
        if !sun.enabled {
            continue;
        }
        
        // Advance time if not paused
        if !sun.cycle_paused && sun.cycle_speed > 0.0 {
            // cycle_speed: hours per real second
            let hours_per_second = sun.cycle_speed / 3600.0;
            sun.time_of_day += time.delta_secs() * hours_per_second;
            sun.time_of_day = sun.time_of_day % 24.0;
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

/// Synchronize the main directional light with the sun position
fn sync_directional_light_with_sun(
    celestial_state: Res<CelestialState>,
    sun_query: Query<&Sun>,
    moon_query: Query<&Moon>,
    mut light_query: Query<(&mut DirectionalLight, &mut Transform), Without<Sun>>,
) {
    // Get sun and moon settings
    let sun = sun_query.iter().find(|s| s.enabled);
    let moon = moon_query.iter().find(|m| m.enabled);
    
    for (mut light, mut transform) in light_query.iter_mut() {
        if celestial_state.is_day {
            // Daytime - use sun
            if let Some(sun) = sun {
                // Point light toward sun (light direction is opposite of sun direction)
                let light_dir = -celestial_state.sun_direction;
                transform.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, light_dir);
                
                // Set light color and intensity
                light.color = celestial_state.sun_color;
                light.illuminance = celestial_state.sun_intensity;
                
                // Shadow settings
                if sun.cast_shadows {
                    // Shadows are handled by Bevy's shadow system
                }
            }
        } else {
            // Nighttime - use moon if bright enough
            if let Some(moon) = moon {
                if moon.cast_shadows && celestial_state.moon_illumination > 0.3 {
                    // Moon provides some directional light
                    let light_dir = -celestial_state.moon_direction;
                    transform.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, light_dir);
                    
                    // Moonlight color (bluish)
                    let moon_color = moon.color;
                    light.color = Color::srgba(moon_color[0], moon_color[1], moon_color[2], moon_color[3]);
                    light.illuminance = moon.current_intensity(celestial_state.sun_elevation) * 1000.0;
                } else {
                    // Very dark night - minimal light
                    light.illuminance = 10.0;
                    light.color = Color::srgba(0.1, 0.1, 0.2, 1.0);
                }
            } else {
                // No moon - very dark
                light.illuminance = 10.0;
                light.color = Color::srgba(0.05, 0.05, 0.1, 1.0);
            }
        }
    }
}

/// Update ambient lighting based on celestial state
fn update_ambient_lighting(
    celestial_state: Res<CelestialState>,
    mut ambient_light: ResMut<AmbientLight>,
) {
    ambient_light.color = celestial_state.ambient_color;
    
    // Scale ambient brightness based on time of day
    let brightness = if celestial_state.is_day {
        0.3 + 0.4 * (celestial_state.sun_elevation / 90.0).max(0.0)
    } else {
        0.02 + 0.08 * celestial_state.moon_illumination
    };
    
    ambient_light.brightness = brightness;
}

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
