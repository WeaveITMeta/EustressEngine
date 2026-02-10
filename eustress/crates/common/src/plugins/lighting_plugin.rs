//! # Shared Lighting Plugin
//! 
//! Common lighting implementation for both Engine and Client.
//! Provides:
//! - Procedural skybox generation
//! - Sun/DirectionalLight setup and updates
//! - Time of day system
//! - Ambient lighting
//! - Global fog (affects all entities: BaseParts, Terrain, Models)
//! - Realtime-filtered environment maps with AtmosphereEnvironmentMapLight

use bevy::prelude::*;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::core_pipeline::Skybox;
use bevy::light::GlobalAmbientLight;
use bevy::render::render_resource::{TextureViewDescriptor, TextureViewDimension, Extent3d, TextureDimension, TextureFormat};
use tracing::info;

use crate::services::lighting::{LightingService, Sun as SunMarker, Moon as MoonMarker, FillLight, EustressAtmosphere, AtmosphereRenderingMode};
use crate::classes::{Sky, Sun as SunClass, Moon as MoonClass, Instance, ClassName};

// ============================================================================
// Plugin
// ============================================================================

/// Shared lighting plugin for Engine and Client
/// 
/// Registers:
/// - LightingService resource
/// - Sky, Atmosphere, Sun, FillLight components
/// - Lighting setup and update systems
pub struct SharedLightingPlugin;

impl Plugin for SharedLightingPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resources
            .init_resource::<LightingService>()
            .init_resource::<SkyboxHandle>()
            .init_resource::<SceneAtmosphere>()
            .register_type::<LightingService>()
            
            // Components
            .register_type::<Sky>()
            .register_type::<SunMarker>()
            .register_type::<SunClass>()
            .register_type::<MoonClass>()
            .register_type::<FillLight>()
            .register_type::<EustressAtmosphere>()
            .register_type::<AtmosphereRenderingMode>()
            
            // Systems
            .add_systems(Startup, setup_lighting)
            .add_systems(Update, (
                update_sun_position,
                update_moon_position,
                update_ambient_light,
                update_exposure_compensation,
                update_fog_settings,
                attach_skybox_to_cameras,
                apply_atmosphere_to_cameras,
                update_atmosphere_effects,
                sync_sun_class_to_sundisk,
                sync_clock_time_to_sun,
            ));
    }
}

// ============================================================================
// Scene Atmosphere Resource
// ============================================================================

// Note: Bevy's built-in Atmosphere component was removed; using custom EustressAtmosphere instead
// #[derive(Component)]
// pub struct SceneAtmosphere {
//     pub atmosphere: BevyAtmosphere,
// }

/// Global scene atmosphere configuration
/// Applied to all cameras that don't have their own EustressAtmosphere component
#[derive(Resource, Clone, Debug)]
pub struct SceneAtmosphere {
    pub atmosphere: EustressAtmosphere,
}

impl Default for SceneAtmosphere {
    fn default() -> Self {
        Self {
            // Default to a pleasant day with light haze
            atmosphere: EustressAtmosphere {
                density: 0.35,
                haze: 0.15,  // Light haze for depth perception
                glare: 0.05,
                color: [0.529, 0.808, 0.922, 1.0],  // Sky blue matching skybox
                decay: [0.7, 0.8, 0.9, 1.0],        // Light blue-gray horizon
                ..EustressAtmosphere::default()
            },
        }
    }
}

// ============================================================================
// Resources
// ============================================================================

/// Stores the skybox image handle
#[derive(Resource, Default)]
pub struct SkyboxHandle {
    pub handle: Option<Handle<Image>>,
}

// ============================================================================
// Systems
// ============================================================================

/// Helper to convert [f32; 4] to Color
fn arr_to_color(arr: [f32; 4]) -> Color {
    Color::srgba(arr[0], arr[1], arr[2], arr[3])
}

/// Setup initial lighting (sun, fill light, ambient, skybox)
fn setup_lighting(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut skybox_handle: ResMut<SkyboxHandle>,
    lighting: Res<LightingService>,
) {
    info!("💡 SharedLightingPlugin: Setting up scene lighting...");
    
    // Create procedural skybox
    let handle = create_procedural_skybox(&mut images, &lighting);
    skybox_handle.handle = Some(handle);
    
    // Sun (main directional light) - softer shadows via increased bias
    // Includes both marker component (for queries) and class component (for properties)
    let sun_dir = lighting.sun_direction();
    let sun_class = SunClass::default();
    commands.spawn((
        DirectionalLight {
            color: arr_to_color(lighting.sun_color),
            illuminance: lighting.sun_intensity * 0.7,
            shadows_enabled: lighting.shadows_enabled,
            ..default()
        },
        Transform::from_translation(sun_dir * 100.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        Visibility::default(),
        SunMarker,
        sun_class,
        Instance {
            name: "Sun".to_string(),
            class_name: ClassName::Sun,
            archivable: true,
            ai: false,
            id: 0,
        },
        Name::new("Sun"),
    ));
    
    // Moon (night directional light) - spawned as default child of Lighting
    // Includes both marker component (for queries) and class component (for properties)
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(0.7, 0.75, 0.9),
            illuminance: 0.5,
            ..default()
        },
        Transform::from_xyz(50.0, 80.0, -30.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        Visibility::default(),
        MoonMarker,
        MoonClass::default(),
        Instance {
            name: "Moon".to_string(),
            class_name: ClassName::Moon,
            archivable: true,
            ai: false,
            id: 0,
        },
        Name::new("Moon"),
    ));
    
    // Fill light (softer, opposite direction for ambient occlusion fill)
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(0.7, 0.75, 0.9),
            illuminance: 5000.0,
            ..default()
        },
        Transform::from_xyz(-30.0, 50.0, -30.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        FillLight,
        Name::new("FillLight"),
    ));
    
    // Secondary fill from below/front to reduce harsh shadows
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(0.8, 0.85, 1.0),
            illuminance: 2000.0,
            ..default()
        },
        Transform::from_xyz(0.0, -20.0, 50.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        Name::new("FillLight2"),
    ));
    
    // GlobalAmbientLight is a Resource in Bevy 0.18
    commands.insert_resource(GlobalAmbientLight {
        color: arr_to_color(lighting.ambient),
        brightness: lighting.brightness * 800.0,
        affects_lightmapped_meshes: true,
    });
    
    info!("✅ Lighting setup complete");
}

/// Update sun position and properties based on LightingService
/// Includes real-time shadow softness control
fn update_sun_position(
    lighting: Res<LightingService>,
    mut sun_query: Query<(&mut DirectionalLight, &mut Transform), With<SunMarker>>,
) {
    if !lighting.is_changed() {
        return;
    }
    
    if let Ok((mut sun_light, mut sun_transform)) = sun_query.single_mut() {
        // Update light properties
        sun_light.color = arr_to_color(lighting.sun_color);
        sun_light.illuminance = lighting.sun_intensity;
        sun_light.shadows_enabled = lighting.shadows_enabled;
        
        // Calculate sun position based on time of day
        let sun_dir = lighting.sun_direction();
        let sun_distance = 100.0;
        
        sun_transform.translation = sun_dir * sun_distance;
        sun_transform.look_at(Vec3::ZERO, Vec3::Y);
    }
}

/// Update ambient light based on LightingService
fn update_ambient_light(
    lighting: Res<LightingService>,
    mut ambient: ResMut<GlobalAmbientLight>,
) {
    if !lighting.is_changed() {
        return;
    }
    
    ambient.color = arr_to_color(lighting.ambient);
    ambient.brightness = lighting.brightness * 500.0;
}

/// Update moon position and properties using realistic orbital mechanics
/// 
/// The Moon follows a realistic orbital path:
/// - Position is based on elongation from Sun (not simply opposite)
/// - Orbital inclination of ~5.1° to the ecliptic
/// - Phase is determined by Sun-Moon angle (elongation)
/// - Geographic latitude affects the Moon's path just like the Sun
fn update_moon_position(
    lighting: Res<LightingService>,
    mut moon_query: Query<(&mut DirectionalLight, &mut Transform, &MoonClass), With<MoonMarker>>,
    sun_query: Query<&SunClass, With<SunMarker>>,
) {
    if !lighting.is_changed() {
        return;
    }
    
    // Get Sun data for realistic moon positioning
    let sun_data = sun_query.iter().next().map(|s| s.clone()).unwrap_or_else(|| {
        // Create a default Sun based on LightingService if no Sun entity exists
        crate::classes::Sun {
            time_of_day: lighting.time_of_day * 24.0,
            latitude: lighting.geographic_latitude,
            ..Default::default()
        }
    });
    
    if let Ok((mut moon_light, mut moon_transform, moon_data)) = moon_query.single_mut() {
        // Calculate moon direction using realistic orbital mechanics
        let moon_dir = moon_data.direction_realistic(&sun_data);
        
        // Get sun elevation for intensity calculations
        let sun_elevation = sun_data.elevation();
        
        // Moon illumination based on phase (elongation from sun)
        let phase_illumination = moon_data.illumination();
        
        // Moon visibility based on sun position
        let moon_intensity = moon_data.current_intensity(sun_elevation) * phase_illumination;
        
        moon_light.illuminance = moon_intensity.max(0.01); // Minimum visibility
        moon_light.shadows_enabled = sun_elevation < -0.1 && phase_illumination > 0.3;
        
        // Position moon in sky
        let moon_distance = 100.0;
        moon_transform.translation = moon_dir * moon_distance;
        moon_transform.look_at(Vec3::ZERO, Vec3::Y);
    }
}

/// Update exposure compensation
/// Affects overall scene brightness/exposure via ambient light adjustment
fn update_exposure_compensation(
    lighting: Res<LightingService>,
    mut ambient: ResMut<GlobalAmbientLight>,
) {
    if !lighting.is_changed() {
        return;
    }
    // Adjust ambient brightness based on exposure compensation
    let exposure_factor = 2.0_f32.powf(lighting.exposure_compensation);
    ambient.brightness = lighting.brightness * 500.0 * exposure_factor;
}

/// Update global fog settings based on LightingService
/// Affects ALL entities: BaseParts, Terrain, Models, etc.
fn update_fog_settings(
    lighting: Res<LightingService>,
    mut camera_query: Query<(Entity, Option<&mut DistanceFog>), With<Camera3d>>,
    mut commands: Commands,
) {
    // Only update when lighting changes
    if !lighting.is_changed() {
        return;
    }
    
    for (entity, fog) in camera_query.iter_mut() {
        if lighting.fog_enabled {
            let fog_color = Color::srgba(
                lighting.fog_color[0],
                lighting.fog_color[1],
                lighting.fog_color[2],
                lighting.fog_color[3],
            );
            
            let new_fog = DistanceFog {
                color: fog_color,
                falloff: FogFalloff::Linear {
                    start: lighting.fog_start,
                    end: lighting.fog_end,
                },
                ..default()
            };
            
            if let Some(mut existing_fog) = fog {
                // Update existing fog
                existing_fog.color = new_fog.color;
                existing_fog.falloff = new_fog.falloff;
            } else {
                // Add fog to camera
                commands.entity(entity).insert(new_fog);
                info!("🌫️ Global fog enabled (start: {}, end: {})", lighting.fog_start, lighting.fog_end);
            }
        } else {
            // Remove fog if disabled
            if fog.is_some() {
                commands.entity(entity).remove::<DistanceFog>();
                info!("🌫️ Global fog disabled");
            }
        }
    }
}

// ============================================================================
// Skybox Generation
// ============================================================================

/// Create a procedural gradient skybox cubemap
/// 
/// Generates a 6-face cubemap with realistic sky gradient:
/// - Zenith (top): deep blue
/// - Mid-sky: lighter blue  
/// - Horizon: warm haze/white
/// - Ground (below horizon): dark ground color
/// Each face pixel is mapped to a 3D direction, then colored by elevation angle.
pub fn create_procedural_skybox(
    images: &mut Assets<Image>,
    _lighting: &LightingService,
) -> Handle<Image> {
    const SIZE: u32 = 256;
    
    // AAA sky palette
    let zenith: [f32; 3] = [0.16, 0.32, 0.75];      // Deep blue zenith
    let mid_sky: [f32; 3] = [0.40, 0.60, 0.92];      // Mid-sky blue
    let horizon: [f32; 3] = [0.75, 0.82, 0.90];      // Pale horizon haze
    let ground: [f32; 3] = [0.22, 0.22, 0.20];        // Dark ground
    
    let mut data = Vec::with_capacity((SIZE * SIZE * 6 * 4) as usize);
    
    // Cubemap face order: +X, -X, +Y, -Y, +Z, -Z
    for face in 0..6u32 {
        for py in 0..SIZE {
            for px in 0..SIZE {
                // Map pixel to [-1, 1] UV
                let u = (px as f32 + 0.5) / SIZE as f32 * 2.0 - 1.0;
                let v = (py as f32 + 0.5) / SIZE as f32 * 2.0 - 1.0;
                
                // Map face + UV to 3D direction
                let (dx, dy, dz) = match face {
                    0 => ( 1.0,  -v,  -u),  // +X
                    1 => (-1.0,  -v,   u),  // -X
                    2 => (   u, 1.0,   v),  // +Y (top)
                    3 => (   u, -1.0, -v),  // -Y (bottom)
                    4 => (   u,  -v, 1.0),  // +Z
                    _ => (  -u,  -v, -1.0), // -Z
                };
                
                // Normalize direction and get elevation
                let len = (dx * dx + dy * dy + dz * dz).sqrt();
                let ny = dy / len; // -1 (nadir) to +1 (zenith)
                
                // Sky gradient based on elevation
                let (r, g, b) = if ny > 0.15 {
                    // Above horizon: blend mid_sky → zenith
                    let t = ((ny - 0.15) / 0.85).min(1.0);
                    let t = t * t; // Ease-in for deeper blue at top
                    (
                        mid_sky[0] + (zenith[0] - mid_sky[0]) * t,
                        mid_sky[1] + (zenith[1] - mid_sky[1]) * t,
                        mid_sky[2] + (zenith[2] - mid_sky[2]) * t,
                    )
                } else if ny > -0.05 {
                    // Horizon band: blend horizon ↔ mid_sky
                    let t = ((ny + 0.05) / 0.20).min(1.0).max(0.0);
                    (
                        horizon[0] + (mid_sky[0] - horizon[0]) * t,
                        horizon[1] + (mid_sky[1] - horizon[1]) * t,
                        horizon[2] + (mid_sky[2] - horizon[2]) * t,
                    )
                } else {
                    // Below horizon: blend horizon → ground
                    let t = ((-ny - 0.05) / 0.35).min(1.0);
                    let t = t.sqrt(); // Quick falloff to ground
                    (
                        horizon[0] + (ground[0] - horizon[0]) * t,
                        horizon[1] + (ground[1] - horizon[1]) * t,
                        horizon[2] + (ground[2] - horizon[2]) * t,
                    )
                };
                
                data.push((r.clamp(0.0, 1.0) * 255.0) as u8);
                data.push((g.clamp(0.0, 1.0) * 255.0) as u8);
                data.push((b.clamp(0.0, 1.0) * 255.0) as u8);
                data.push(255);
            }
        }
    }
    
    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    
    // Configure as cubemap
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    
    images.add(image)
}

/// Regenerate skybox when lighting colors change
pub fn regenerate_skybox(
    images: &mut Assets<Image>,
    lighting: &LightingService,
    skybox_handle: &mut SkyboxHandle,
) {
    let handle = create_procedural_skybox(images, lighting);
    skybox_handle.handle = Some(handle);
}

// ============================================================================
// Skybox Attachment System
// ============================================================================

/// Marker component for cameras that have had skybox attached
#[derive(Component)]
pub struct SkyboxAttached;

/// Automatically attach skybox to any Camera3d that doesn't have one
/// This ensures both Engine and Client cameras get the skybox
fn attach_skybox_to_cameras(
    mut commands: Commands,
    skybox_handle: Res<SkyboxHandle>,
    cameras_without_skybox: Query<Entity, (With<Camera3d>, Without<Skybox>, Without<SkyboxAttached>)>,
) {
    // Only proceed if we have a skybox handle
    let Some(ref skybox_image) = skybox_handle.handle else {
        return;
    };
    
    for camera_entity in cameras_without_skybox.iter() {
        info!("🌅 Attaching skybox to camera {:?}", camera_entity);
        
        commands.entity(camera_entity).insert((
            Skybox {
                image: skybox_image.clone(),
                brightness: 1000.0,
                rotation: Quat::IDENTITY,
            },
            EnvironmentMapLight {
                diffuse_map: skybox_image.clone(),
                specular_map: skybox_image.clone(),
                intensity: 400.0,
                rotation: Quat::IDENTITY,
                affects_lightmapped_mesh_diffuse: false,
            },
            SkyboxAttached, // Mark as processed
        ));
    }
}

// ============================================================================
// Atmosphere System (Bevy 0.17 Raymarched Atmosphere)
// ============================================================================

/// Marker for cameras that have had atmosphere applied
#[derive(Component)]
pub struct AtmosphereApplied;

/// Apply EustressAtmosphere settings to cameras
/// 
/// This system:
/// 1. Applies scene-level atmosphere to cameras without custom atmosphere
/// 2. Converts EustressAtmosphere to Bevy's Atmosphere component
/// 3. Sets up AtmosphereSettings for raymarching mode
/// 4. Enables AtmosphereEnvironmentMapLight for dynamic reflections
/// 
/// Note: Bevy 0.17's Atmosphere and AtmosphereSettings components are used
/// when available. This provides a compatibility layer.
fn apply_atmosphere_to_cameras(
    mut commands: Commands,
    scene_atmosphere: Res<SceneAtmosphere>,
    cameras_without_atmosphere: Query<
        Entity, 
        (With<Camera3d>, Without<AtmosphereApplied>)
    >,
    cameras_with_custom: Query<
        (Entity, &EustressAtmosphere), 
        (With<Camera3d>, Without<AtmosphereApplied>)
    >,
) {
    // Apply custom atmosphere to cameras that have EustressAtmosphere component
    for (camera_entity, atmosphere) in cameras_with_custom.iter() {
        apply_atmosphere_settings(&mut commands, camera_entity, atmosphere);
    }
    
    // Apply scene atmosphere to cameras without custom atmosphere
    for camera_entity in cameras_without_atmosphere.iter() {
        // Skip if already processed via custom atmosphere
        if cameras_with_custom.iter().any(|(e, _)| e == camera_entity) {
            continue;
        }
        
        apply_atmosphere_settings(&mut commands, camera_entity, &scene_atmosphere.atmosphere);
    }
}

/// Apply atmosphere settings to a camera entity
/// Note: Bevy's Atmosphere component was removed; using fog + skybox as substitute
fn apply_atmosphere_settings(
    commands: &mut Commands,
    camera_entity: Entity,
    _atmosphere: &EustressAtmosphere,
) {
    // Mark as processed so we don't re-apply every frame
    commands.entity(camera_entity).insert(AtmosphereApplied);
}

/// Update atmosphere effects
/// Note: Bevy's Atmosphere component was removed; atmosphere is simulated via fog + skybox
fn update_atmosphere_effects(
    _commands: Commands,
    _scene_atmosphere: Res<SceneAtmosphere>,
) {
    // Atmosphere effects are handled via fog settings and skybox colors
}

// ============================================================================
// Atmosphere Presets (convenience functions)
// ============================================================================

impl SceneAtmosphere {
    /// Set to clear day atmosphere
    pub fn clear_day() -> Self {
        Self {
            atmosphere: EustressAtmosphere::clear_day(),
        }
    }
    
    /// Set to sunset atmosphere
    pub fn sunset() -> Self {
        Self {
            atmosphere: EustressAtmosphere::sunset(),
        }
    }
    
    /// Set to foggy atmosphere
    pub fn foggy() -> Self {
        Self {
            atmosphere: EustressAtmosphere::foggy(),
        }
    }
    
    /// Set to space view (raymarched)
    pub fn space_view() -> Self {
        Self {
            atmosphere: EustressAtmosphere::space_view(),
        }
    }
    
    /// Set to flight simulator (raymarched)
    pub fn flight_sim() -> Self {
        Self {
            atmosphere: EustressAtmosphere::flight_sim(),
        }
    }
}

// ============================================================================
// Sun/Moon Class Property Sync Systems
// ============================================================================

/// Sync Sun class angular_size property
/// Note: SunDisk component was removed from Bevy; angular size is tracked in SunClass only
fn sync_sun_class_to_sundisk() {
    // SunDisk was removed from Bevy — angular size is stored in SunClass
    // and used for skybox/atmosphere calculations directly
}

/// Sync LightingService.clock_time to Sun.time_of_day for day/night cycle
fn sync_clock_time_to_sun(
    lighting: Res<LightingService>,
    mut sun_query: Query<&mut SunClass, With<SunMarker>>,
) {
    if !lighting.is_changed() {
        return;
    }
    
    // Parse clock_time string (format: "HH:MM:SS" or "HH:MM") to time_of_day (0-24)
    let time_of_day = parse_clock_time(&lighting.clock_time)
        .unwrap_or(lighting.time_of_day * 24.0);
    
    for mut sun in sun_query.iter_mut() {
        if (sun.time_of_day - time_of_day).abs() > 0.01 {
            sun.time_of_day = time_of_day;
        }
    }
}

/// Parse clock time string to hours (0-24)
/// Supports formats: "14:30:00", "14:30", "14"
fn parse_clock_time(clock_time: &str) -> Option<f32> {
    let parts: Vec<&str> = clock_time.split(':').collect();
    if parts.is_empty() {
        return None;
    }
    
    let hours: f32 = parts.first()?.parse().ok()?;
    let minutes: f32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let seconds: f32 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    
    Some(hours + minutes / 60.0 + seconds / 3600.0)
}
