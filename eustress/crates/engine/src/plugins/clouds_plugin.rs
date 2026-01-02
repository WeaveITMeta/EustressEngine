// ============================================================================
// Clouds Plugin - Volumetric Cloud Rendering System
// ============================================================================
//
// This plugin manages AAA-quality procedural cloud rendering:
// - Multi-layer volumetric cloud billboards with noise-based shapes
// - Wind-driven cloud movement with turbulence
// - Hemisphere-based coverage distribution
// - Time-of-day tinting (sunrise/sunset colors)
// - Cloud shadows on terrain
// - Weather system integration (syncs with LightingService)
// - Precipitation effects (rain, snow)
//
// Table of Contents:
// 1. Plugin Definition
// 2. Resources (CloudState, WeatherState, GlobalWindDirection)
// 3. Components (CloudParticle, CloudLayer, PrecipitationParticle)
// 4. Systems (weather_sync, cloud_management, precipitation, lighting_integration)
// 5. Helper Functions
// 6. Presets
// ============================================================================

use bevy::prelude::*;
use eustress_common::classes::{Clouds, CloudCoverage, CloudLayerType, Instance, ClassName};
use eustress_common::services::LightingService;

// ============================================================================
// 1. Plugin Definition
// ============================================================================

/// Plugin for volumetric cloud rendering and weather system
pub struct CloudsPlugin;

impl Plugin for CloudsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CloudState>()
            .init_resource::<WeatherState>()
            .init_resource::<GlobalWindDirection>()
            .add_systems(Update, (
                sync_weather_with_lighting,
                update_global_wind,
                update_weather_transitions,
                update_cloud_movement,
                update_cloud_colors,
                manage_cloud_particles,
                update_precipitation,
                update_fog_from_weather,
            ).chain());
    }
}

// ============================================================================
// 2. Resources
// ============================================================================

/// Global cloud rendering state
#[derive(Resource, Default)]
pub struct CloudState {
    /// Total number of active cloud particles
    pub particle_count: usize,
    /// Current wind offset for cloud UV scrolling
    pub wind_offset: Vec2,
    /// Time accumulator for animations
    pub time: f32,
    /// Whether clouds need regeneration
    pub needs_regeneration: bool,
    /// Current cloud layer entities
    pub cloud_layers: Vec<Entity>,
}

/// Weather types that affect clouds, fog, and lighting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WeatherType {
    #[default]
    Clear,
    PartlyCloudy,
    Cloudy,
    Overcast,
    Fog,
    Rain,
    HeavyRain,
    Thunderstorm,
    Snow,
    Blizzard,
}

impl WeatherType {
    /// Parse weather type from string (matches AtmosphereSettings.weather)
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "clear" => Self::Clear,
            "partly_cloudy" | "partlycloudy" | "partly cloudy" => Self::PartlyCloudy,
            "cloudy" => Self::Cloudy,
            "overcast" => Self::Overcast,
            "fog" | "foggy" => Self::Fog,
            "rain" | "rainy" => Self::Rain,
            "heavy_rain" | "heavyrain" | "heavy rain" => Self::HeavyRain,
            "storm" | "thunderstorm" => Self::Thunderstorm,
            "snow" | "snowy" => Self::Snow,
            "blizzard" => Self::Blizzard,
            _ => Self::Clear,
        }
    }
    
    /// Get cloud coverage for this weather type (0.0 - 1.0)
    pub fn cloud_coverage(&self) -> f32 {
        match self {
            Self::Clear => 0.1,
            Self::PartlyCloudy => 0.35,
            Self::Cloudy => 0.6,
            Self::Overcast => 0.9,
            Self::Fog => 0.7,
            Self::Rain => 0.8,
            Self::HeavyRain => 0.95,
            Self::Thunderstorm => 1.0,
            Self::Snow => 0.85,
            Self::Blizzard => 1.0,
        }
    }
    
    /// Get cloud density for this weather type (0.0 - 1.0)
    pub fn cloud_density(&self) -> f32 {
        match self {
            Self::Clear => 0.2,
            Self::PartlyCloudy => 0.4,
            Self::Cloudy => 0.6,
            Self::Overcast => 0.85,
            Self::Fog => 0.5,
            Self::Rain => 0.75,
            Self::HeavyRain => 0.9,
            Self::Thunderstorm => 0.95,
            Self::Snow => 0.7,
            Self::Blizzard => 0.95,
        }
    }
    
    /// Get fog density for this weather type (0.0 - 1.0)
    pub fn fog_density(&self) -> f32 {
        match self {
            Self::Clear => 0.0,
            Self::PartlyCloudy => 0.0,
            Self::Cloudy => 0.05,
            Self::Overcast => 0.15,
            Self::Fog => 0.8,
            Self::Rain => 0.25,
            Self::HeavyRain => 0.4,
            Self::Thunderstorm => 0.35,
            Self::Snow => 0.3,
            Self::Blizzard => 0.7,
        }
    }
    
    /// Get precipitation intensity (0.0 = none, 1.0 = heavy)
    pub fn precipitation_intensity(&self) -> f32 {
        match self {
            Self::Clear | Self::PartlyCloudy | Self::Cloudy | Self::Overcast | Self::Fog => 0.0,
            Self::Rain => 0.4,
            Self::HeavyRain => 0.8,
            Self::Thunderstorm => 0.9,
            Self::Snow => 0.5,
            Self::Blizzard => 1.0,
        }
    }
    
    /// Is this weather type precipitation as snow?
    pub fn is_snow(&self) -> bool {
        matches!(self, Self::Snow | Self::Blizzard)
    }
    
    /// Get ambient light multiplier (darker during storms)
    pub fn ambient_multiplier(&self) -> f32 {
        match self {
            Self::Clear => 1.0,
            Self::PartlyCloudy => 0.95,
            Self::Cloudy => 0.85,
            Self::Overcast => 0.7,
            Self::Fog => 0.75,
            Self::Rain => 0.65,
            Self::HeavyRain => 0.5,
            Self::Thunderstorm => 0.4,
            Self::Snow => 0.8,
            Self::Blizzard => 0.45,
        }
    }
    
    /// Get cloud color tint for this weather
    pub fn cloud_color(&self) -> [f32; 4] {
        match self {
            Self::Clear => [1.0, 1.0, 1.0, 1.0],
            Self::PartlyCloudy => [0.98, 0.98, 1.0, 1.0],
            Self::Cloudy => [0.9, 0.9, 0.92, 1.0],
            Self::Overcast => [0.75, 0.75, 0.78, 1.0],
            Self::Fog => [0.85, 0.85, 0.88, 1.0],
            Self::Rain => [0.6, 0.6, 0.65, 1.0],
            Self::HeavyRain => [0.45, 0.45, 0.5, 1.0],
            Self::Thunderstorm => [0.3, 0.3, 0.35, 1.0],
            Self::Snow => [0.9, 0.92, 0.95, 1.0],
            Self::Blizzard => [0.8, 0.82, 0.88, 1.0],
        }
    }
    
    /// Get wind speed multiplier
    pub fn wind_multiplier(&self) -> f32 {
        match self {
            Self::Clear => 1.0,
            Self::PartlyCloudy => 1.2,
            Self::Cloudy => 1.3,
            Self::Overcast => 1.1,
            Self::Fog => 0.5,
            Self::Rain => 1.5,
            Self::HeavyRain => 2.0,
            Self::Thunderstorm => 2.5,
            Self::Snow => 1.3,
            Self::Blizzard => 3.0,
        }
    }
}

/// Global weather state resource
#[derive(Resource)]
pub struct WeatherState {
    /// Current weather type
    pub current_weather: WeatherType,
    /// Target weather type (for transitions)
    pub target_weather: WeatherType,
    /// Transition progress (0.0 = current, 1.0 = target)
    pub transition_progress: f32,
    /// Transition speed (0.0 - 1.0 per second)
    pub transition_speed: f32,
    /// Current interpolated cloud coverage
    pub cloud_coverage: f32,
    /// Current interpolated cloud density
    pub cloud_density: f32,
    /// Current interpolated fog density
    pub fog_density: f32,
    /// Current precipitation intensity
    pub precipitation_intensity: f32,
    /// Is precipitation snow?
    pub is_snow: bool,
    /// Current ambient light multiplier
    pub ambient_multiplier: f32,
    /// Current cloud color
    pub cloud_color: [f32; 4],
    /// Current wind multiplier
    pub wind_multiplier: f32,
    /// Lightning flash timer (for thunderstorms)
    pub lightning_timer: f32,
    /// Lightning flash intensity (0.0 - 1.0)
    pub lightning_intensity: f32,
}

impl Default for WeatherState {
    fn default() -> Self {
        Self {
            current_weather: WeatherType::Clear,
            target_weather: WeatherType::Clear,
            transition_progress: 1.0,
            transition_speed: 0.1, // 10 seconds for full transition
            cloud_coverage: 0.1,
            cloud_density: 0.2,
            fog_density: 0.0,
            precipitation_intensity: 0.0,
            is_snow: false,
            ambient_multiplier: 1.0,
            cloud_color: [1.0, 1.0, 1.0, 1.0],
            wind_multiplier: 1.0,
            lightning_timer: 0.0,
            lightning_intensity: 0.0,
        }
    }
}

impl WeatherState {
    /// Set weather with optional transition
    pub fn set_weather(&mut self, weather: WeatherType, instant: bool) {
        self.target_weather = weather;
        if instant {
            self.current_weather = weather;
            self.transition_progress = 1.0;
            self.apply_weather_values();
        } else {
            self.transition_progress = 0.0;
        }
    }
    
    /// Apply current weather values (called when transition completes)
    fn apply_weather_values(&mut self) {
        self.cloud_coverage = self.current_weather.cloud_coverage();
        self.cloud_density = self.current_weather.cloud_density();
        self.fog_density = self.current_weather.fog_density();
        self.precipitation_intensity = self.current_weather.precipitation_intensity();
        self.is_snow = self.current_weather.is_snow();
        self.ambient_multiplier = self.current_weather.ambient_multiplier();
        self.cloud_color = self.current_weather.cloud_color();
        self.wind_multiplier = self.current_weather.wind_multiplier();
    }
}

/// Global wind direction resource (shared with other systems)
#[derive(Resource)]
pub struct GlobalWindDirection {
    /// Wind direction in degrees (0 = North, 90 = East)
    pub direction: f32,
    /// Base wind speed in studs per second
    pub base_speed: f32,
    /// Current wind speed (affected by weather)
    pub speed: f32,
    /// Wind direction as normalized vector
    pub direction_vec: Vec3,
    /// Turbulence amount (0.0 - 1.0)
    pub turbulence: f32,
    /// Gust timer for wind gusts
    pub gust_timer: f32,
    /// Current gust strength
    pub gust_strength: f32,
}

impl Default for GlobalWindDirection {
    fn default() -> Self {
        Self {
            direction: 45.0,
            base_speed: 10.0,
            speed: 10.0,
            direction_vec: Vec3::new(0.707, 0.0, 0.707), // Northeast
            turbulence: 0.2,
            gust_timer: 0.0,
            gust_strength: 0.0,
        }
    }
}

// ============================================================================
// 3. Components
// ============================================================================

/// Individual cloud particle component
#[derive(Component)]
pub struct CloudParticle {
    /// Base position before wind offset
    pub base_position: Vec3,
    /// Size of this cloud particle
    pub size: Vec2,
    /// Opacity (0-1)
    pub opacity: f32,
    /// Animation phase offset
    pub phase: f32,
    /// Which cloud entity this belongs to
    pub parent_cloud: Entity,
    /// Layer index (for multi-layer clouds)
    pub layer: u8,
    /// Noise offset for shape variation
    pub noise_offset: Vec2,
}

/// Marker for cloud billboard meshes
#[derive(Component)]
pub struct CloudBillboard;

/// Precipitation particle component
#[derive(Component)]
pub struct PrecipitationParticle {
    /// Velocity of this particle
    pub velocity: Vec3,
    /// Lifetime remaining
    pub lifetime: f32,
    /// Is this snow (vs rain)?
    pub is_snow: bool,
    /// Size of the particle
    pub size: f32,
}

// ============================================================================
// 4. Systems
// ============================================================================

/// Sync weather state with LightingService
fn sync_weather_with_lighting(
    lighting: Option<Res<LightingService>>,
    mut weather_state: ResMut<WeatherState>,
) {
    // If LightingService exists, we could sync weather from it
    // For now, weather is managed independently but affects lighting
    if let Some(_lighting) = lighting {
        // Weather state is primary - it drives lighting changes
        // This system ensures weather is initialized
    }
}

/// Update weather transitions over time
fn update_weather_transitions(
    time: Res<Time>,
    mut weather_state: ResMut<WeatherState>,
) {
    // Handle weather transition
    if weather_state.transition_progress < 1.0 {
        let transition_speed = weather_state.transition_speed;
        weather_state.transition_progress += time.delta_secs() * transition_speed;
        weather_state.transition_progress = weather_state.transition_progress.min(1.0);
        
        let t = weather_state.transition_progress;
        let current = weather_state.current_weather;
        let target = weather_state.target_weather;
        
        // Interpolate all weather values
        weather_state.cloud_coverage = lerp(current.cloud_coverage(), target.cloud_coverage(), t);
        weather_state.cloud_density = lerp(current.cloud_density(), target.cloud_density(), t);
        weather_state.fog_density = lerp(current.fog_density(), target.fog_density(), t);
        weather_state.precipitation_intensity = lerp(current.precipitation_intensity(), target.precipitation_intensity(), t);
        weather_state.ambient_multiplier = lerp(current.ambient_multiplier(), target.ambient_multiplier(), t);
        weather_state.wind_multiplier = lerp(current.wind_multiplier(), target.wind_multiplier(), t);
        
        // Interpolate cloud color
        let current_color = current.cloud_color();
        let target_color = target.cloud_color();
        weather_state.cloud_color = [
            lerp(current_color[0], target_color[0], t),
            lerp(current_color[1], target_color[1], t),
            lerp(current_color[2], target_color[2], t),
            lerp(current_color[3], target_color[3], t),
        ];
        
        // Snow state switches at midpoint
        if t > 0.5 {
            weather_state.is_snow = target.is_snow();
        }
        
        // Complete transition
        if weather_state.transition_progress >= 1.0 {
            weather_state.current_weather = target;
        }
    }
    
    // Handle lightning for thunderstorms
    let is_thunderstorm = weather_state.current_weather == WeatherType::Thunderstorm;
    if is_thunderstorm {
        weather_state.lightning_timer -= time.delta_secs();
        
        if weather_state.lightning_timer <= 0.0 {
            // Random lightning flash
            let random_val = (time.elapsed_secs() * 1000.0) as u32 % 100;
            if random_val < 5 { // 5% chance per frame when timer expires
                weather_state.lightning_intensity = 0.8 + (random_val as f32 / 100.0) * 0.2;
                weather_state.lightning_timer = 0.1; // Flash duration
            } else {
                weather_state.lightning_timer = 2.0 + (random_val as f32 / 100.0) * 8.0; // 2-10 seconds between flashes
            }
        }
        
        // Decay lightning intensity
        weather_state.lightning_intensity *= 0.85;
    } else {
        weather_state.lightning_intensity = 0.0;
    }
}

/// Update global wind direction from Clouds components
fn update_global_wind(
    time: Res<Time>,
    mut wind: ResMut<GlobalWindDirection>,
    weather_state: Res<WeatherState>,
    clouds_query: Query<&Clouds>,
) {
    // Use the first enabled cloud's wind settings
    for clouds in clouds_query.iter() {
        if clouds.enabled {
            wind.direction = clouds.wind_direction;
            wind.base_speed = clouds.wind_speed;
            wind.direction_vec = clouds.wind_direction_vec();
            break;
        }
    }
    
    // Apply weather multiplier to wind speed
    wind.speed = wind.base_speed * weather_state.wind_multiplier;
    
    // Update wind gusts
    wind.gust_timer -= time.delta_secs();
    if wind.gust_timer <= 0.0 {
        // Random gust
        let random_val = (time.elapsed_secs() * 1000.0) as u32 % 100;
        wind.gust_strength = (random_val as f32 / 100.0) * weather_state.wind_multiplier * 0.5;
        wind.gust_timer = 1.0 + (random_val as f32 / 100.0) * 4.0;
    }
    wind.gust_strength *= 0.95; // Decay gusts
    
    // Set turbulence based on weather
    wind.turbulence = match weather_state.current_weather {
        WeatherType::Clear => 0.1,
        WeatherType::PartlyCloudy => 0.15,
        WeatherType::Cloudy => 0.2,
        WeatherType::Overcast => 0.15,
        WeatherType::Fog => 0.05,
        WeatherType::Rain => 0.3,
        WeatherType::HeavyRain => 0.4,
        WeatherType::Thunderstorm => 0.6,
        WeatherType::Snow => 0.25,
        WeatherType::Blizzard => 0.5,
    };
}

/// Update cloud particle positions based on wind
fn update_cloud_movement(
    time: Res<Time>,
    wind: Res<GlobalWindDirection>,
    mut cloud_state: ResMut<CloudState>,
    mut particle_query: Query<(&mut Transform, &CloudParticle)>,
) {
    // Update time accumulator
    cloud_state.time += time.delta_secs();
    
    // Calculate wind offset with gusts
    let effective_speed = wind.speed + wind.gust_strength * wind.base_speed;
    let wind_delta = wind.direction_vec * effective_speed * time.delta_secs();
    cloud_state.wind_offset.x += wind_delta.x;
    cloud_state.wind_offset.y += wind_delta.z;
    
    // Wrap wind offset to prevent float precision issues
    if cloud_state.wind_offset.x.abs() > 10000.0 {
        cloud_state.wind_offset.x = cloud_state.wind_offset.x % 10000.0;
    }
    if cloud_state.wind_offset.y.abs() > 10000.0 {
        cloud_state.wind_offset.y = cloud_state.wind_offset.y % 10000.0;
    }
    
    // Move cloud particles
    for (mut transform, particle) in particle_query.iter_mut() {
        // Apply wind movement with turbulence
        let turbulence_x = (cloud_state.time * 0.3 + particle.noise_offset.x).sin() * wind.turbulence * 20.0;
        let turbulence_z = (cloud_state.time * 0.25 + particle.noise_offset.y).cos() * wind.turbulence * 20.0;
        
        transform.translation = particle.base_position + Vec3::new(
            cloud_state.wind_offset.x + turbulence_x,
            0.0,
            cloud_state.wind_offset.y + turbulence_z,
        );
        
        // Add subtle vertical bobbing (more for higher layers)
        let bob_speed = 0.3 + particle.layer as f32 * 0.1;
        let bob_amplitude = 2.0 + particle.layer as f32 * 1.0;
        let bob = (cloud_state.time * bob_speed + particle.phase).sin() * bob_amplitude;
        transform.translation.y += bob;
    }
}

/// Update cloud colors based on time of day and weather
fn update_cloud_colors(
    clouds_query: Query<&Clouds>,
    weather_state: Res<WeatherState>,
    celestial_state: Option<Res<super::celestial_plugin::CelestialState>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    particle_query: Query<(&CloudParticle, &MeshMaterial3d<StandardMaterial>)>,
) {
    // Get active cloud settings
    let Some(clouds) = clouds_query.iter().find(|c| c.enabled) else {
        return;
    };
    
    // Start with weather-based cloud color
    let weather_color = weather_state.cloud_color;
    
    // Calculate cloud color based on time of day
    let base_color = if clouds.time_of_day_tinting {
        if let Some(celestial) = celestial_state {
            // Tint clouds based on sun position
            let sun_color = celestial.sun_color;
            let tint_strength = if celestial.sun_elevation < 20.0 && celestial.sun_elevation > -10.0 {
                // Sunrise/sunset - strong tinting
                0.5
            } else {
                0.1
            };
            
            // Combine weather color with sun tinting
            Color::srgba(
                weather_color[0] * clouds.color[0] * (1.0 - tint_strength) + sun_color.to_srgba().red * tint_strength,
                weather_color[1] * clouds.color[1] * (1.0 - tint_strength) + sun_color.to_srgba().green * tint_strength,
                weather_color[2] * clouds.color[2] * (1.0 - tint_strength) + sun_color.to_srgba().blue * tint_strength,
                clouds.color[3],
            )
        } else {
            Color::srgba(
                weather_color[0] * clouds.color[0],
                weather_color[1] * clouds.color[1],
                weather_color[2] * clouds.color[2],
                clouds.color[3],
            )
        }
    } else {
        Color::srgba(
            weather_color[0] * clouds.color[0],
            weather_color[1] * clouds.color[1],
            weather_color[2] * clouds.color[2],
            clouds.color[3],
        )
    };
    
    // Apply lightning flash
    let lightning_boost = weather_state.lightning_intensity;
    
    // Update particle materials
    for (particle, material_handle) in particle_query.iter() {
        if let Some(material) = materials.get_mut(&material_handle.0) {
            let mut color = base_color.with_alpha(particle.opacity);
            
            // Add lightning flash (brightens clouds)
            if lightning_boost > 0.0 {
                let srgba = color.to_srgba();
                color = Color::srgba(
                    (srgba.red + lightning_boost).min(1.0),
                    (srgba.green + lightning_boost).min(1.0),
                    (srgba.blue + lightning_boost * 1.2).min(1.0), // Slightly blue tint
                    srgba.alpha,
                );
            }
            
            material.base_color = color;
        }
    }
}

/// Manage cloud particle spawning and despawning based on weather
fn manage_cloud_particles(
    mut commands: Commands,
    mut cloud_state: ResMut<CloudState>,
    weather_state: Res<WeatherState>,
    clouds_query: Query<(Entity, &Clouds)>,
    clouds_changed: Query<(Entity, &Clouds), Changed<Clouds>>,
    existing_particles: Query<(Entity, &CloudParticle)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Process changed clouds
    for (cloud_entity, clouds) in clouds_changed.iter() {
        if !clouds.enabled {
            // Despawn all particles for this cloud
            for (particle_entity, particle) in existing_particles.iter() {
                if particle.parent_cloud == cloud_entity {
                    commands.entity(particle_entity).despawn();
                }
            }
            continue;
        }
        
        // Calculate desired particle count based on weather-modified density and coverage
        let effective_density = clouds.density * weather_state.cloud_density;
        let effective_coverage = clouds.coverage * weather_state.cloud_coverage;
        let target_count = ((effective_density * effective_coverage * 150.0) as usize).max(10);
        
        // Count existing particles for this cloud
        let current_count = existing_particles.iter()
            .filter(|(_, p)| p.parent_cloud == cloud_entity)
            .count();
        
        if current_count < target_count {
            // Spawn more particles
            let to_spawn = target_count - current_count;
            spawn_cloud_particles(
                &mut commands,
                &mut meshes,
                &mut materials,
                cloud_entity,
                clouds,
                to_spawn,
                current_count,
            );
        } else if current_count > target_count {
            // Remove excess particles
            let to_remove = current_count - target_count;
            let mut removed = 0;
            for (particle_entity, particle) in existing_particles.iter() {
                if particle.parent_cloud == cloud_entity && removed < to_remove {
                    commands.entity(particle_entity).despawn();
                    removed += 1;
                }
            }
        }
        
        cloud_state.particle_count = target_count;
    }
    
    // Also check if weather changed significantly (even without Clouds component change)
    // This handles dynamic weather transitions
    for (cloud_entity, clouds) in clouds_query.iter() {
        if !clouds.enabled {
            continue;
        }
        
        let effective_density = clouds.density * weather_state.cloud_density;
        let effective_coverage = clouds.coverage * weather_state.cloud_coverage;
        let target_count = ((effective_density * effective_coverage * 150.0) as usize).max(10);
        
        let current_count = existing_particles.iter()
            .filter(|(_, p)| p.parent_cloud == cloud_entity)
            .count();
        
        // Only adjust if difference is significant (>20%)
        let diff = (target_count as i32 - current_count as i32).abs();
        if diff > (target_count as i32 / 5).max(5) {
            if current_count < target_count {
                let to_spawn = (target_count - current_count).min(10); // Spawn gradually
                spawn_cloud_particles(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    cloud_entity,
                    clouds,
                    to_spawn,
                    current_count,
                );
            } else if current_count > target_count {
                let to_remove = (current_count - target_count).min(10); // Remove gradually
                let mut removed = 0;
                for (particle_entity, particle) in existing_particles.iter() {
                    if particle.parent_cloud == cloud_entity && removed < to_remove {
                        commands.entity(particle_entity).despawn();
                        removed += 1;
                    }
                }
            }
        }
        
        cloud_state.particle_count = current_count;
    }
}

/// Update precipitation particles (rain/snow)
fn update_precipitation(
    mut commands: Commands,
    time: Res<Time>,
    weather_state: Res<WeatherState>,
    wind: Res<GlobalWindDirection>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut particles: Query<(Entity, &mut Transform, &mut PrecipitationParticle)>,
    camera: Query<&Transform, (With<Camera3d>, Without<PrecipitationParticle>)>,
) {
    let intensity = weather_state.precipitation_intensity;
    
    // Get camera position for spawning around player
    let camera_pos = camera.iter().next().map(|t| t.translation).unwrap_or(Vec3::ZERO);
    
    // Update existing particles
    for (entity, mut transform, mut particle) in particles.iter_mut() {
        particle.lifetime -= time.delta_secs();
        
        if particle.lifetime <= 0.0 || transform.translation.y < -10.0 {
            commands.entity(entity).despawn();
            continue;
        }
        
        // Apply velocity with wind influence
        let wind_effect = if particle.is_snow { 0.8 } else { 0.3 };
        transform.translation += particle.velocity * time.delta_secs();
        transform.translation.x += wind.direction_vec.x * wind.speed * wind_effect * time.delta_secs();
        transform.translation.z += wind.direction_vec.z * wind.speed * wind_effect * time.delta_secs();
        
        // Snow swirls
        if particle.is_snow {
            let swirl = (time.elapsed_secs() * 2.0 + transform.translation.y * 0.1).sin() * 0.5;
            transform.translation.x += swirl * time.delta_secs();
        }
    }
    
    // Spawn new particles if there's precipitation
    if intensity > 0.0 {
        let particle_count = particles.iter().count();
        let target_count = (intensity * 500.0) as usize;
        
        if particle_count < target_count {
            let to_spawn = ((target_count - particle_count) / 10).max(1).min(20);
            
            // Create mesh and material for precipitation
            let mesh = if weather_state.is_snow {
                meshes.add(Rectangle::new(0.3, 0.3))
            } else {
                meshes.add(Rectangle::new(0.05, 0.5))
            };
            
            let material = materials.add(StandardMaterial {
                base_color: if weather_state.is_snow {
                    Color::srgba(1.0, 1.0, 1.0, 0.8)
                } else {
                    Color::srgba(0.7, 0.75, 0.85, 0.4)
                },
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                double_sided: true,
                ..default()
            });
            
            for i in 0..to_spawn {
                // Random position around camera
                let random_seed = (time.elapsed_secs() * 1000.0 + i as f32 * 137.0) as u32;
                let offset_x = ((random_seed % 1000) as f32 / 1000.0 - 0.5) * 100.0;
                let offset_z = ((random_seed / 1000 % 1000) as f32 / 1000.0 - 0.5) * 100.0;
                let height = 50.0 + ((random_seed / 1000000 % 100) as f32 / 100.0) * 30.0;
                
                let velocity = if weather_state.is_snow {
                    Vec3::new(0.0, -5.0 - (random_seed % 100) as f32 / 50.0, 0.0)
                } else {
                    Vec3::new(0.0, -20.0 - (random_seed % 100) as f32 / 10.0, 0.0)
                };
                
                commands.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(Vec3::new(
                        camera_pos.x + offset_x,
                        camera_pos.y + height,
                        camera_pos.z + offset_z,
                    )),
                    PrecipitationParticle {
                        velocity,
                        lifetime: 10.0,
                        is_snow: weather_state.is_snow,
                        size: if weather_state.is_snow { 0.3 } else { 0.05 },
                    },
                ));
            }
        }
    }
}

/// Update fog based on weather state
fn update_fog_from_weather(
    weather_state: Res<WeatherState>,
    mut fog_settings: Query<&mut DistanceFog>,
) {
    for mut fog in fog_settings.iter_mut() {
        // Interpolate fog based on weather
        let target_density = weather_state.fog_density;
        
        if target_density > 0.01 {
            fog.falloff = FogFalloff::Linear {
                start: 50.0 / (target_density + 0.1),
                end: 500.0 / (target_density + 0.1),
            };
            
            // Fog color based on weather
            fog.color = match weather_state.current_weather {
                WeatherType::Fog => Color::srgba(0.85, 0.85, 0.88, 1.0),
                WeatherType::Rain | WeatherType::HeavyRain => Color::srgba(0.6, 0.62, 0.68, 1.0),
                WeatherType::Thunderstorm => Color::srgba(0.4, 0.42, 0.48, 1.0),
                WeatherType::Snow => Color::srgba(0.9, 0.92, 0.95, 1.0),
                WeatherType::Blizzard => Color::srgba(0.95, 0.95, 0.98, 1.0),
                _ => Color::srgba(0.8, 0.85, 0.9, 1.0),
            };
        }
    }
}

// ============================================================================
// 5. Helper Functions
// ============================================================================

/// Linear interpolation helper
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Spawn cloud particles for a Clouds component
fn spawn_cloud_particles(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    cloud_entity: Entity,
    clouds: &Clouds,
    count: usize,
    start_index: usize,
) {
    // Create multiple mesh sizes for variety
    let base_size = 50.0 * clouds.spread;
    let mesh_small = meshes.add(Rectangle::new(base_size * 0.6, base_size * 0.4));
    let mesh_medium = meshes.add(Rectangle::new(base_size, base_size * 0.6));
    let mesh_large = meshes.add(Rectangle::new(base_size * 1.5, base_size * 0.8));
    
    // Create cloud material with proper blending
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(clouds.color[0], clouds.color[1], clouds.color[2], 0.85),
        alpha_mode: AlphaMode::Blend,
        unlit: true, // Clouds are self-illuminated for now
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    
    // Spawn particles distributed across the sky
    for i in 0..count {
        let index = start_index + i;
        let position = generate_cloud_position(clouds, index, count.max(50));
        let size = generate_cloud_size(clouds, index);
        let opacity = generate_cloud_opacity(clouds, index);
        let layer = ((index * 31) % 3) as u8;
        
        // Select mesh based on layer type and variation
        let mesh = match (clouds.layer_type, index % 3) {
            (CloudLayerType::Cirrus, _) => mesh_large.clone(),
            (CloudLayerType::Stratus, _) => mesh_large.clone(),
            (CloudLayerType::Cumulonimbus, 0) => mesh_large.clone(),
            (CloudLayerType::Cumulonimbus, _) => mesh_medium.clone(),
            (_, 0) => mesh_small.clone(),
            (_, 1) => mesh_medium.clone(),
            (_, _) => mesh_large.clone(),
        };
        
        // Generate noise offset for turbulence variation
        let noise_offset = Vec2::new(
            (index as f32 * 0.7).sin() * 100.0,
            (index as f32 * 1.3).cos() * 100.0,
        );
        
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(position)
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), // Face up
            CloudParticle {
                base_position: position,
                size,
                opacity,
                phase: (index as f32) * 0.7,
                parent_cloud: cloud_entity,
                layer,
                noise_offset,
            },
            CloudBillboard,
        ));
    }
}

/// Generate a cloud position based on coverage mode
fn generate_cloud_position(clouds: &Clouds, index: usize, total: usize) -> Vec3 {
    use std::f32::consts::PI;
    
    // Base distribution on a hemisphere
    let golden_ratio = (1.0 + 5.0_f32.sqrt()) / 2.0;
    let theta = 2.0 * PI * (index as f32) / golden_ratio;
    let phi = (1.0 - 2.0 * (index as f32 + 0.5) / total as f32).acos();
    
    // Convert to Cartesian (on upper hemisphere)
    let base_x = phi.sin() * theta.cos();
    let base_y = phi.cos().abs(); // Keep on upper hemisphere
    let base_z = phi.sin() * theta.sin();
    
    // Apply coverage mode bias
    let (bias_x, bias_z) = match clouds.coverage_mode {
        CloudCoverage::Full => (0.0, 0.0),
        CloudCoverage::Northern => (0.0, -clouds.coverage_bias),
        CloudCoverage::Southern => (0.0, clouds.coverage_bias),
        CloudCoverage::Eastern => (clouds.coverage_bias, 0.0),
        CloudCoverage::Western => (-clouds.coverage_bias, 0.0),
        CloudCoverage::Horizon => {
            // Push toward horizon (reduce Y)
            return Vec3::new(
                base_x * 1000.0 * clouds.spread,
                clouds.altitude * 0.3,
                base_z * 1000.0 * clouds.spread,
            );
        }
        CloudCoverage::Zenith => {
            // Push toward zenith (increase Y component)
            return Vec3::new(
                base_x * 500.0 * clouds.spread * (1.0 - clouds.coverage_bias),
                clouds.altitude,
                base_z * 500.0 * clouds.spread * (1.0 - clouds.coverage_bias),
            );
        }
        CloudCoverage::Scattered => {
            // Add randomness
            let noise = ((index * 7919) % 1000) as f32 / 1000.0 - 0.5;
            (noise * 0.3, noise * 0.3)
        }
    };
    
    Vec3::new(
        (base_x + bias_x) * 1000.0 * clouds.spread,
        clouds.altitude + base_y * clouds.thickness,
        (base_z + bias_z) * 1000.0 * clouds.spread,
    )
}

/// Generate cloud particle size based on layer type
fn generate_cloud_size(clouds: &Clouds, index: usize) -> Vec2 {
    let base_size = match clouds.layer_type {
        CloudLayerType::Cumulus => Vec2::new(80.0, 50.0),
        CloudLayerType::Cirrus => Vec2::new(150.0, 20.0),
        CloudLayerType::Stratus => Vec2::new(200.0, 30.0),
        CloudLayerType::Cumulonimbus => Vec2::new(100.0, 150.0),
        CloudLayerType::Altocumulus => Vec2::new(60.0, 40.0),
    };
    
    // Add variation
    let variation = 0.7 + ((index * 31337) % 1000) as f32 / 1000.0 * 0.6;
    base_size * variation * clouds.spread
}

/// Generate cloud opacity based on softness and position
fn generate_cloud_opacity(clouds: &Clouds, index: usize) -> f32 {
    let base_opacity = 0.6 + clouds.density * 0.3;
    let variation = ((index * 12345) % 1000) as f32 / 1000.0 * clouds.softness * 0.3;
    (base_opacity - variation).clamp(0.3, 0.95)
}

/// Spawn a default Clouds entity
pub fn spawn_clouds(commands: &mut Commands) -> Entity {
    commands.spawn((
        Instance {
            name: "Clouds".to_string(),
            class_name: ClassName::Clouds,
            archivable: true,
            id: 0,
        },
        Clouds::default(),
    )).id()
}

/// Create preset cloud configurations
pub mod presets {
    use super::*;
    
    pub fn clear_sky(commands: &mut Commands) -> Entity {
        commands.spawn((
            Instance {
                name: "Clouds".to_string(),
                class_name: ClassName::Clouds,
                archivable: true,
                id: 0,
            },
            Clouds::clear(),
        )).id()
    }
    
    pub fn partly_cloudy(commands: &mut Commands) -> Entity {
        commands.spawn((
            Instance {
                name: "Clouds".to_string(),
                class_name: ClassName::Clouds,
                archivable: true,
                id: 0,
            },
            Clouds::partly_cloudy(),
        )).id()
    }
    
    pub fn overcast(commands: &mut Commands) -> Entity {
        commands.spawn((
            Instance {
                name: "Clouds".to_string(),
                class_name: ClassName::Clouds,
                archivable: true,
                id: 0,
            },
            Clouds::overcast(),
        )).id()
    }
    
    pub fn stormy(commands: &mut Commands) -> Entity {
        commands.spawn((
            Instance {
                name: "Clouds".to_string(),
                class_name: ClassName::Clouds,
                archivable: true,
                id: 0,
            },
            Clouds::stormy(),
        )).id()
    }
}
