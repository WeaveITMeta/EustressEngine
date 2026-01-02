//! # Lighting Service Types
//! 
//! Shared data types for scene lighting across Engine and Client.
//! 
//! ## Classes
//! 
//! - `LightingService`: Scene lighting configuration (like Eustress's Lighting)
//! - `Atmosphere`: Atmospheric effects
//! - `Sky`: Skybox configuration

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// LightingService Resource
// ============================================================================

/// LightingService - manages scene lighting (like Eustress's Lighting service)
#[derive(Resource, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct LightingService {
    // --- Time ---
    /// Time of day (0.0 = midnight, 0.5 = noon, 1.0 = midnight)
    pub time_of_day: f32,
    /// Clock time string (e.g., "14:30:00")
    pub clock_time: String,
    /// Geographic latitude for sun position (-90 to 90)
    pub geographic_latitude: f32,
    
    // --- Ambient ---
    /// Ambient light color
    pub ambient: [f32; 4],
    /// Overall brightness multiplier
    pub brightness: f32,
    /// Outdoor ambient color
    pub outdoor_ambient: [f32; 4],
    
    // --- Fog ---
    /// Fog color
    pub fog_color: [f32; 4],
    /// Fog start distance
    pub fog_start: f32,
    /// Fog end distance  
    pub fog_end: f32,
    /// Is fog enabled
    pub fog_enabled: bool,
    
    // --- Sun ---
    /// Sun/directional light color
    pub sun_color: [f32; 4],
    /// Sun intensity (lux)
    pub sun_intensity: f32,
    /// Sun angular radius (degrees)
    pub sun_angular_radius: f32,
    
    // --- Shadows ---
    /// Are shadows enabled globally
    pub shadows_enabled: bool,
    /// Shadow softness (0-1)
    pub shadow_softness: f32,
    
    // --- Sky ---
    /// Sky color (zenith)
    pub sky_color: [f32; 4],
    /// Horizon color
    pub horizon_color: [f32; 4],
    
    // --- Exposure ---
    /// Exposure compensation
    pub exposure_compensation: f32,
    /// Environment diffuse scale
    pub environment_diffuse_scale: f32,
    /// Environment specular scale
    pub environment_specular_scale: f32,
}

impl Default for LightingService {
    fn default() -> Self {
        Self {
            // Time
            time_of_day: 0.5, // Noon
            clock_time: "12:00:00".to_string(),
            geographic_latitude: 45.0,
            
            // Ambient
            ambient: [0.4, 0.45, 0.5, 1.0],
            brightness: 1.0,
            outdoor_ambient: [0.5, 0.55, 0.6, 1.0],
            
            // Fog
            fog_color: [0.8, 0.85, 0.9, 1.0],
            fog_start: 100.0,
            fog_end: 500.0,
            fog_enabled: false,
            
            // Sun
            sun_color: [1.0, 0.98, 0.95, 1.0],
            sun_intensity: 15000.0,
            sun_angular_radius: 0.5,
            
            // Shadows
            shadows_enabled: true,
            shadow_softness: 0.5,
            
            // Sky
            sky_color: [0.4, 0.6, 0.9, 1.0],
            horizon_color: [0.7, 0.8, 0.95, 1.0],
            
            // Exposure
            exposure_compensation: 0.0,
            environment_diffuse_scale: 1.0,
            environment_specular_scale: 1.0,
        }
    }
}

impl LightingService {
    /// Set time of day (0.0 - 1.0)
    pub fn with_time(mut self, time: f32) -> Self {
        self.time_of_day = time.clamp(0.0, 1.0);
        self
    }
    
    /// Convert time_of_day to clock string
    pub fn update_clock_time(&mut self) {
        let hours = (self.time_of_day * 24.0) as u32;
        let minutes = ((self.time_of_day * 24.0 * 60.0) % 60.0) as u32;
        self.clock_time = format!("{:02}:{:02}:00", hours, minutes);
    }
    
    /// Get sun direction based on time of day
    pub fn sun_direction(&self) -> Vec3 {
        let angle = self.time_of_day * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        let height = angle.sin().max(0.05);
        Vec3::new(angle.cos(), height, 0.3).normalize()
    }
}

// ============================================================================
// Atmosphere (Bevy 0.17 Raymarched Atmosphere + Roblox-like Properties)
// ============================================================================

/// Atmosphere rendering mode (Bevy 0.17+)
/// 
/// Controls how the atmosphere is rendered:
/// - `LookupTexture`: Fast, good for ground-level outdoor scenes (default)
/// - `Raymarched`: Accurate, ideal for space views and flight simulators
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum AtmosphereRenderingMode {
    /// Fast lookup-texture based rendering (default)
    /// Great for ground level and broad outdoor scenes
    /// Less accurate at long distances, softer shadows
    #[default]
    LookupTexture,
    
    /// Raymarched rendering for accurate atmosphere
    /// Ideal for cinematic shots, planets from space, flight simulators
    /// More accurate lighting, sharper shadows, but slower
    Raymarched,
}

/// Comprehensive Atmosphere component (Roblox-like + Bevy 0.17 features)
/// 
/// Combines Roblox Studio's Atmosphere properties with Bevy's new raymarched
/// atmosphere and realtime-filtered environment maps.
/// 
/// # Bevy 0.17 Features
/// - `AtmosphereMode::Raymarched` for space/flight views
/// - `AtmosphereEnvironmentMapLight` for dynamic environment lighting
/// - `GeneratedEnvironmentMapLight` for procedural skybox reflections
/// 
/// # Example
/// ```rust
/// commands.spawn((
///     Camera3d::default(),
///     EustressAtmosphere::default(),
/// ));
/// ```
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct EustressAtmosphere {
    // === Roblox-like Properties ===
    
    /// Density of the atmosphere (0.0 - 1.0)
    /// Higher values create thicker, hazier atmosphere
    /// Roblox default: 0.395
    pub density: f32,
    
    /// Offset for density calculation
    /// Affects how density changes with altitude
    /// Roblox default: 0.0
    pub offset: f32,
    
    /// Primary atmosphere color (tints the sky)
    /// Roblox default: RGB(199, 170, 107) - warm sunset tone
    pub color: [f32; 4],
    
    /// Decay/haze color at the horizon
    /// Affects the color of distant objects and horizon
    /// Roblox default: RGB(92, 60, 13) - warm brown
    pub decay: [f32; 4],
    
    /// Glare intensity around the sun (0.0 - 1.0)
    /// Creates a bright halo effect around the sun
    /// Roblox default: 0.0
    pub glare: f32,
    
    /// Haze amount (0.0 - 1.0)
    /// Adds atmospheric haze/fog effect
    /// Roblox default: 0.0
    pub haze: f32,
    
    // === Bevy 0.17 Atmosphere Properties ===
    
    /// Rendering mode: LookupTexture (fast) or Raymarched (accurate)
    /// Use Raymarched for space views or flight simulators
    #[serde(default)]
    pub rendering_mode: AtmosphereRenderingMode,
    
    /// Maximum raymarching samples (only used in Raymarched mode)
    /// Higher = more accurate but slower (default: 32)
    /// Range: 8 - 128
    #[serde(default = "default_sky_max_samples")]
    pub sky_max_samples: u32,
    
    /// Planet radius in meters (affects atmosphere curvature)
    /// Earth default: 6,371,000 meters
    #[serde(default = "default_planet_radius")]
    pub planet_radius: f32,
    
    /// Atmosphere height in meters (thickness of atmosphere layer)
    /// Earth default: ~100,000 meters
    #[serde(default = "default_atmosphere_height")]
    pub atmosphere_height: f32,
    
    /// Rayleigh scattering coefficient (affects blue sky color)
    /// Higher values = more blue scattering
    #[serde(default = "default_rayleigh_coefficient")]
    pub rayleigh_coefficient: [f32; 3],
    
    /// Mie scattering coefficient (affects sun glare/haze)
    /// Higher values = more pronounced sun glare
    #[serde(default = "default_mie_coefficient")]
    pub mie_coefficient: f32,
    
    /// Mie scattering direction (-1 to 1)
    /// Negative = backscatter, Positive = forward scatter
    #[serde(default = "default_mie_direction")]
    pub mie_direction: f32,
    
    // === Environment Map Settings ===
    
    /// Enable realtime-filtered environment map for reflections
    /// Uses Bevy's GeneratedEnvironmentMapLight
    #[serde(default = "default_true")]
    pub environment_map_enabled: bool,
    
    /// Environment map intensity for ambient lighting
    #[serde(default = "default_environment_intensity")]
    pub environment_intensity: f32,
    
    /// Enable atmosphere-based environment lighting
    /// Uses Bevy's AtmosphereEnvironmentMapLight
    #[serde(default = "default_true")]
    pub atmosphere_environment_light: bool,
}

// Default value functions for serde
fn default_sky_max_samples() -> u32 { 32 }
fn default_planet_radius() -> f32 { 6_371_000.0 }
fn default_atmosphere_height() -> f32 { 100_000.0 }
fn default_rayleigh_coefficient() -> [f32; 3] { [5.5e-6, 13.0e-6, 22.4e-6] }
fn default_mie_coefficient() -> f32 { 21e-6 }
fn default_mie_direction() -> f32 { 0.758 }
fn default_true() -> bool { true }
fn default_environment_intensity() -> f32 { 1.0 }

impl Default for EustressAtmosphere {
    fn default() -> Self {
        Self {
            // Bevy Earth-like atmosphere defaults
            density: 0.35,                        // Moderate density for clear sky
            offset: 0.0,
            color: [0.4, 0.6, 1.0, 1.0],          // Blue sky (Rayleigh scattering)
            decay: [0.3, 0.3, 0.3, 1.0],          // Neutral ground albedo
            glare: 0.0,
            haze: 0.0,
            
            // Bevy 0.17 atmosphere defaults
            rendering_mode: AtmosphereRenderingMode::LookupTexture,
            sky_max_samples: 32,
            planet_radius: 6_371_000.0,          // Earth radius in meters
            atmosphere_height: 100_000.0,        // 100km atmosphere
            rayleigh_coefficient: [5.5e-6, 13.0e-6, 22.4e-6], // Earth Rayleigh
            mie_coefficient: 21e-6,              // Earth Mie
            mie_direction: 0.758,                // Forward scattering
            
            // Environment map defaults
            environment_map_enabled: true,
            environment_intensity: 1.0,
            atmosphere_environment_light: true,
        }
    }
}

impl EustressAtmosphere {
    /// Create a clear day atmosphere (minimal haze)
    pub fn clear_day() -> Self {
        Self {
            density: 0.3,
            haze: 0.0,
            glare: 0.0,
            color: [0.5, 0.7, 1.0, 1.0],         // Blue sky
            decay: [0.9, 0.85, 0.8, 1.0],        // Light horizon
            ..Default::default()
        }
    }
    
    /// Create a sunset/sunrise atmosphere
    pub fn sunset() -> Self {
        Self {
            density: 0.5,
            haze: 0.2,
            glare: 0.3,
            color: [1.0, 0.6, 0.3, 1.0],         // Orange sky
            decay: [0.8, 0.3, 0.1, 1.0],         // Red horizon
            ..Default::default()
        }
    }
    
    /// Create a foggy/overcast atmosphere
    pub fn foggy() -> Self {
        Self {
            density: 0.8,
            haze: 0.7,
            glare: 0.0,
            color: [0.7, 0.7, 0.75, 1.0],        // Gray sky
            decay: [0.6, 0.6, 0.65, 1.0],        // Gray horizon
            ..Default::default()
        }
    }
    
    /// Create a space view atmosphere (for viewing planet from orbit)
    pub fn space_view() -> Self {
        Self {
            density: 0.2,
            haze: 0.0,
            glare: 0.0,
            color: [0.3, 0.5, 1.0, 1.0],
            decay: [0.1, 0.2, 0.4, 1.0],
            rendering_mode: AtmosphereRenderingMode::Raymarched,
            sky_max_samples: 64,                 // Higher quality for space
            ..Default::default()
        }
    }
    
    /// Create a flight simulator atmosphere
    pub fn flight_sim() -> Self {
        Self {
            density: 0.35,
            haze: 0.1,
            glare: 0.1,
            color: [0.5, 0.7, 1.0, 1.0],
            decay: [0.8, 0.85, 0.9, 1.0],
            rendering_mode: AtmosphereRenderingMode::Raymarched,
            sky_max_samples: 48,
            ..Default::default()
        }
    }
    
    /// Create an alien planet atmosphere with custom colors
    pub fn alien_planet(sky_color: [f32; 4], horizon_color: [f32; 4]) -> Self {
        Self {
            density: 0.5,
            haze: 0.2,
            glare: 0.1,
            color: sky_color,
            decay: horizon_color,
            rendering_mode: AtmosphereRenderingMode::Raymarched,
            sky_max_samples: 48,
            ..Default::default()
        }
    }
    
    /// Set rendering mode
    pub fn with_rendering_mode(mut self, mode: AtmosphereRenderingMode) -> Self {
        self.rendering_mode = mode;
        self
    }
    
    /// Set raymarching quality (samples)
    pub fn with_quality(mut self, samples: u32) -> Self {
        self.sky_max_samples = samples.clamp(8, 128);
        self
    }
    
    /// Enable/disable environment map lighting
    pub fn with_environment_map(mut self, enabled: bool) -> Self {
        self.environment_map_enabled = enabled;
        self
    }
}

// Note: Atmosphere component is now in classes.rs for Explorer visibility
// Use EustressAtmosphere for camera-attached atmosphere effects

// ============================================================================
// Sky Settings (runtime configuration)
// ============================================================================

/// Sky runtime settings (complements classes::Sky for runtime state)
/// Use classes::Sky for the entity component, this for service-level config
#[derive(Resource, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct SkySettings {
    /// Use procedural sky instead of texture
    pub procedural: bool,
    /// Sun visible in sky
    pub sun_visible: bool,
    /// Moon visible in sky
    pub moon_visible: bool,
    /// Stars visible
    pub stars_visible: bool,
    /// Celestial bodies scale
    pub celestial_scale: f32,
}

impl Default for SkySettings {
    fn default() -> Self {
        Self {
            procedural: true,
            sun_visible: true,
            moon_visible: true,
            stars_visible: true,
            celestial_scale: 1.0,
        }
    }
}

// ============================================================================
// Light Markers
// ============================================================================

/// Marks the sun directional light
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Sun;

/// Marks a fill light
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct FillLight;

/// Marks the moon light (if separate from sun)
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Moon;
