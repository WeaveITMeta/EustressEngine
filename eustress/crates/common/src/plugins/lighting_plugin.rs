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
use bevy::pbr::{Atmosphere as BevyAtmosphere, ScatteringMedium, DistanceFog, FogFalloff};
use bevy::core_pipeline::Skybox;
use bevy::light::{GlobalAmbientLight, light_consts::lux, CascadeShadowConfigBuilder, VolumetricLight, SunDisk};
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
                // Regenerate skybox after sun position updates so the sun disk tracks time of day
                regenerate_skybox_on_sun_change.after(update_sun_position),
                attach_skybox_to_cameras,
                apply_atmosphere_to_cameras.after(attach_skybox_to_cameras),
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
    info!("💡 SharedLightingPlugin: Setting up Bevy Atmosphere lighting...");

    // Keep procedural skybox as fallback / for editor preview
    let handle = create_procedural_skybox(&mut images, &lighting);
    skybox_handle.handle = Some(handle);

    // Shadow cascade configuration
    let cascade_shadow_config = CascadeShadowConfigBuilder {
        num_cascades: 4,
        minimum_distance: 0.1,
        maximum_distance: 2048.0,
        first_cascade_far_bound: 16.0,
        overlap_proportion: 0.3,
        ..default()
    }
    .build();

    // Sun — uses RAW_SUNLIGHT illuminance for physically correct atmosphere scattering
    // SunDisk renders the visible sun disc in Bevy's Atmosphere shader
    let sun_dir = lighting.sun_direction();
    let sun_class = SunClass::default();
    commands.spawn((
        DirectionalLight {
            color: arr_to_color(lighting.sun_color),
            illuminance: lux::RAW_SUNLIGHT,
            shadows_enabled: true,
            shadow_depth_bias: 0.02,
            shadow_normal_bias: 1.8,
            ..default()
        },
        SunDisk {
            angular_size: sun_class.angular_size.to_radians(),
            intensity: 1.0,
        },
        Transform::from_translation(sun_dir * 100.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        Visibility::default(),
        VolumetricLight, // God rays from sun
        cascade_shadow_config,
        SunMarker,
        sun_class,
        Instance {
            name: "Sun".to_string(),
            class_name: ClassName::Star,
            archivable: true,
            ai: false,
            uuid: String::new(),
            id: 0,
        },
        Name::new("Sun"),
    ));

    // Moon — dimmer directional light opposite sun
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(0.7, 0.75, 0.9),
            illuminance: 500.0,
            shadows_enabled: false,
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
            uuid: String::new(),
            id: 0,
        },
        Name::new("Moon"),
    ));

    // No fill lights — Bevy's Atmosphere + Environment Map handles ambient
    commands.insert_resource(GlobalAmbientLight::NONE);

    info!("✅ Bevy Atmosphere lighting setup complete");
}

/// Update sun position and properties based on LightingService
/// Includes real-time shadow softness control
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
            if lighting.time_of_day > 1.0 { lighting.time_of_day -= 1.0; }
        }
    } else {
        lighting.bypass_change_detection();
    }

    if let Ok((mut sun_light, mut sun_transform)) = sun_query.single_mut() {
        // Use SunClass direction if available (proper solar math with latitude),
        // otherwise fall back to LightingService's simple formula.
        let sun_dir = sun_class_query.iter().next()
            .map(|sc| sc.direction())
            .unwrap_or_else(|| lighting.sun_direction());

        sun_light.color = arr_to_color(lighting.sun_color);
        let elevation = sun_dir.y;
        let intensity_factor = elevation.max(0.0).powf(0.4);
        sun_light.illuminance = lighting.sun_intensity * intensity_factor;
        sun_light.shadows_enabled = elevation > 0.05;

        let sun_distance = 100.0;
        sun_transform.translation = sun_dir * sun_distance;
        sun_transform.look_at(Vec3::ZERO, Vec3::Y);
    }
}

/// Update ambient light based on LightingService and sun elevation.
/// At night, ambient drops to near-zero so the sky darkens properly.
fn update_ambient_light(
    lighting: Res<LightingService>,
    sun_class_query: Query<&SunClass, With<SunMarker>>,
    mut ambient: ResMut<GlobalAmbientLight>,
) {
    let sun_dir = sun_class_query.iter().next()
        .map(|sc| sc.direction())
        .unwrap_or_else(|| lighting.sun_direction());
    let sun_y = sun_dir.y;

    // Night factor: 1.0 at day, fades to 0.02 at night
    let night_factor = if sun_y > 0.1 { 1.0 }
        else if sun_y > -0.15 { ((sun_y + 0.15) / 0.25).clamp(0.02, 1.0) }
        else { 0.02 };

    ambient.color = arr_to_color(lighting.ambient);
    ambient.brightness = lighting.brightness * 500.0 * night_factor;
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
    if !lighting.is_changed() && !lighting.cycle_enabled {
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
    mut camera_query: Query<(Entity, &Camera, Option<&mut DistanceFog>), With<Camera3d>>,
    mut commands: Commands,
) {
    // Only update when lighting changes
    if !lighting.is_changed() {
        return;
    }
    
    for (entity, camera, fog) in camera_query.iter_mut() {
        // Only apply fog to the main 3D camera, not Slint overlay or other cameras
        if camera.order != 0 { continue; }
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
    lighting: &LightingService,
) -> Handle<Image> {
    create_procedural_skybox_with_sun(images, lighting, None)
}

/// Inner skybox builder — accepts an optional explicit sun direction.
/// When `sun_dir_override` is `Some`, it is used instead of `lighting.sun_direction()`
/// so the live `SunClass::direction()` can be passed in for accurate tracking.
pub fn create_procedural_skybox_with_sun(
    images: &mut Assets<Image>,
    lighting: &LightingService,
    sun_dir_override: Option<Vec3>,
) -> Handle<Image> {
    const SIZE: u32 = 1024;
    
    // Time-of-day sky palette — lerp between day and night based on sun elevation
    let sun_dir = sun_dir_override.unwrap_or_else(|| lighting.sun_direction());
    let night_t = (-sun_dir.y).clamp(0.0, 0.3) / 0.3; // 0=day, 1=deep night

    // Day palette
    let day_zenith: [f32; 3] = [0.16, 0.32, 0.75];
    let day_mid: [f32; 3] = [0.40, 0.60, 0.92];
    let day_horizon: [f32; 3] = [0.75, 0.82, 0.90];

    // Night palette — very dark for stars to show through atmosphere
    let night_zenith: [f32; 3] = [0.01, 0.01, 0.03];
    let night_mid: [f32; 3] = [0.02, 0.02, 0.06];
    let night_horizon: [f32; 3] = [0.04, 0.04, 0.08];

    let lerp3 = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
    };

    let zenith = lerp3(day_zenith, night_zenith, night_t);
    let mid_sky = lerp3(day_mid, night_mid, night_t);
    let horizon = lerp3(day_horizon, night_horizon, night_t);
    let ground: [f32; 3] = [0.22 * (1.0 - night_t * 0.7), 0.22 * (1.0 - night_t * 0.7), 0.20 * (1.0 - night_t * 0.7)];
    
    let sun_angular_radius = lighting.sun_angular_radius.to_radians().max(0.005); // degrees → radians
    let sun_color: [f32; 3] = [lighting.sun_color[0], lighting.sun_color[1], lighting.sun_color[2]];
    // Corona extends 4x the sun disc radius for a soft glow
    let corona_radius = sun_angular_radius * 4.0;
    
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
                let nx = dx / len;
                let ny = dy / len; // -1 (nadir) to +1 (zenith)
                let nz = dz / len;
                
                // Sky gradient based on elevation
                let (mut r, mut g, mut b) = if ny > 0.15 {
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
                
                // Sun/Moon discs are rendered by the analytical SunDiscShader
                // (resolution-independent, pixel-perfect at any distance).
                // Cubemap only contains sky gradient + stars.

                // Stars — visible at night, fade in during twilight
                let pixel_faces_sky = ny > -0.02;
                let sun_below = sun_dir.y < 0.0;
                if sun_below && pixel_faces_sky {
                    // Hash for pseudo-random star positions across the sphere
                    let h1 = (nx * 127.1 + ny * 311.7 + nz * 74.7).sin() * 43758.5453;
                    let star_seed = h1.fract().abs();
                    let h2 = (nx * 269.5 + ny * 183.3 + nz * 421.1).sin() * 28947.7231;
                    let star_seed2 = h2.fract().abs();
                    let h3 = (nx * 419.2 + ny * 67.3 + nz * 253.9).sin() * 17654.3219;
                    let star_seed3 = h3.fract().abs();

                    // Star density: ~0.8% bright stars + ~2% dim stars
                    let star_threshold = if star_seed3 > 0.5 { 0.992 } else { 0.980 };
                    if star_seed > star_threshold {
                        // Fade in as sun goes below horizon
                        let night_factor = (-sun_dir.y).clamp(0.0, 0.2) * 5.0; // 0→1 over 0.2 sun_dir.y range
                        let twinkle = 0.4 + 0.6 * star_seed2;

                        // Brightness varies by star "magnitude"
                        let magnitude = if star_seed > 0.998 { 1.5 } // bright stars
                            else if star_seed > 0.995 { 1.0 }        // medium stars
                            else { 0.5 };                              // dim stars
                        let star_brightness = night_factor * twinkle * magnitude;

                        // Color: blue-white (hot) to warm yellow (cool)
                        let warmth = star_seed2;
                        r = r + star_brightness * (0.85 + warmth * 0.15);
                        g = g + star_brightness * (0.88 + (1.0 - warmth) * 0.12);
                        b = b + star_brightness * (1.0 - warmth * 0.1);
                    }
                }

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
    cameras_without_skybox: Query<Entity, (With<Camera3d>, Without<Skybox>, Without<SkyboxAttached>, Without<NoAtmosphere>)>,
) {
    // Only proceed if we have a skybox handle
    let Some(ref skybox_image) = skybox_handle.handle else {
        return;
    };

    for camera_entity in cameras_without_skybox.iter() {
        // Skip cameras that will get Atmosphere (Atmosphere replaces Skybox for sky rendering)
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
            // SSAO requires Msaa::Off which conflicts with MSAA anti-aliasing.
            // MSAA is more important for visual quality, so SSAO is disabled.
            // bevy::pbr::ScreenSpaceAmbientOcclusion::default(),
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

/// Marker to exclude a camera from atmosphere/skybox systems (e.g. overlay cameras)
#[derive(Component)]
pub struct NoAtmosphere;

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
    mut mediums: ResMut<Assets<ScatteringMedium>>,
    mut cached_medium: Local<Option<Handle<ScatteringMedium>>>,
    cameras_without_atmosphere: Query<
        Entity,
        (With<Camera3d>, Without<AtmosphereApplied>, Without<NoAtmosphere>)
    >,
    cameras_with_custom: Query<
        (Entity, &EustressAtmosphere),
        (With<Camera3d>, Without<AtmosphereApplied>, Without<NoAtmosphere>)
    >,
) {
    // Early exit if no cameras need atmosphere
    if cameras_without_atmosphere.is_empty() && cameras_with_custom.is_empty() {
        return;
    }

    // Create the scattering medium once and cache the handle
    let medium_handle = cached_medium.get_or_insert_with(|| {
        mediums.add(ScatteringMedium::earthlike(256, 256))
    }).clone();

    // Apply custom atmosphere to cameras that have EustressAtmosphere component
    for (camera_entity, atmosphere) in cameras_with_custom.iter() {
        apply_atmosphere_settings(&mut commands, camera_entity, atmosphere, &medium_handle);
    }

    // Apply scene atmosphere to cameras without custom atmosphere
    for camera_entity in cameras_without_atmosphere.iter() {
        if cameras_with_custom.iter().any(|(e, _)| e == camera_entity) {
            continue;
        }
        apply_atmosphere_settings(&mut commands, camera_entity, &scene_atmosphere.atmosphere, &medium_handle);
    }
}

/// Apply Bevy's built-in Atmosphere to a camera entity.
/// The Atmosphere shader renders raymarched sky + sun disc (via SunDisk on DirectionalLight).
fn apply_atmosphere_settings(
    commands: &mut Commands,
    camera_entity: Entity,
    _atmosphere: &EustressAtmosphere,
    medium_handle: &Handle<ScatteringMedium>,
) {
    commands.entity(camera_entity).insert((
        BevyAtmosphere::earthlike(medium_handle.clone()),
        AtmosphereApplied,
    ));
    info!("🌍 Applied Bevy Atmosphere to camera {:?}", camera_entity);
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

/// Regenerate the procedural skybox cubemap whenever the sun position changes.
/// This ensures the sun disk in the skybox tracks the time-of-day cycle.
fn regenerate_skybox_on_sun_change(
    lighting: Res<LightingService>,
    sun_query: Query<&SunClass, With<SunMarker>>,
    changed_sun_query: Query<&SunClass, (With<SunMarker>, Changed<SunClass>)>,
    mut images: ResMut<Assets<Image>>,
    mut skybox_handle: ResMut<SkyboxHandle>,
    mut camera_query: Query<&mut Skybox, With<Camera3d>>,
) {
    // Rebuild when SunClass changes or LightingService changes.
    // Throttle to every 60 frames (~1 second) to avoid regenerating 512x512x6 cubemap every frame.
    static REGEN_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let frame = REGEN_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let has_changes = !changed_sun_query.is_empty() || lighting.is_changed();
    if !has_changes || frame % 60 != 0 {
        return;
    }
    
    // Get the live sun direction from SunClass (uses proper latitude/time_of_day solar math)
    // and the current sun color for accurate disc rendering
    let (sun_dir_override, sun_color_override) = if let Some(sun) = sun_query.iter().next() {
        let dir = sun.direction();
        let color = sun.current_color();
        (Some(dir), Some(color))
    } else {
        (None, None)
    };
    
    // Build snapshot with overridden sun color if available
    let mut lighting_snapshot = lighting.clone();
    if let Some(color) = sun_color_override {
        lighting_snapshot.sun_color = color;
    }
    
    let new_handle = create_procedural_skybox_with_sun(&mut images, &lighting_snapshot, sun_dir_override);
    skybox_handle.handle = Some(new_handle.clone());
    
    // Update all cameras that already have a Skybox component
    for mut skybox in camera_query.iter_mut() {
        skybox.image = new_handle.clone();
    }
}

/// Sync Sun class angular_size property to SunDisk component in real-time
fn sync_sun_class_to_sundisk(
    mut sun_query: Query<(&SunClass, &mut SunDisk), Changed<SunClass>>,
) {
    for (sun_class, mut sun_disk) in sun_query.iter_mut() {
        let new_angular_size = sun_class.angular_size.to_radians();
        if (sun_disk.angular_size - new_angular_size).abs() > 0.001 {
            sun_disk.angular_size = new_angular_size;
            info!("☀️ Sun angular_size synced: {:.1}° → {:.4} rad",
                  sun_class.angular_size, new_angular_size);
        }
    }
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
