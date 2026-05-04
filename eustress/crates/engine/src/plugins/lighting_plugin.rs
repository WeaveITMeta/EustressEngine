//! # Lighting Plugin
//! 
//! Uses the shared lighting plugin from eustress_common.
//! Adds engine-specific light class registrations.
//! Hydrates file-loaded Lighting/ entities with real ECS components.
//! 
//! ## Architecture
//! Each Space owns its lighting via `Lighting/*.instance.toml` files.
//! The file loader spawns them as bare `Instance` entities. This plugin's
//! `hydrate_lighting_entities` system detects freshly-loaded lighting
//! class entities (Star, Moon, Sky, Atmosphere) and attaches the real
//! Bevy components (DirectionalLight, SunMarker, SunClass, etc.).
//! On Space switch, all entities are despawned; the new Space's TOMLs
//! are re-loaded and re-hydrated, preserving per-Space lighting config.

use bevy::prelude::*;
use bevy::light::{light_consts::lux, CascadeShadowConfigBuilder, VolumetricLight, SunDisk};
use eustress_common::classes::{
    ClassName, Instance, EustressPointLight, EustressSpotLight, SurfaceLight, Terrain, Atmosphere,
    Sun as SunClass, Moon as MoonClass, Sky,
};
use eustress_common::services::lighting::{Sun as SunMarker, Moon as MoonMarker, EustressAtmosphere, LightingService};

// Re-export shared plugin
pub use eustress_common::plugins::lighting_plugin::{
    SharedLightingPlugin, SkyboxHandle,
    create_procedural_skybox, regenerate_skybox,
};

/// Component to track which service an entity belongs to (for Explorer)
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[reflect(Component)]
pub struct LightingServiceOwner;

pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        // Use the shared lighting plugin (sun, ambient, skybox)
        app.add_plugins(SharedLightingPlugin);
        
        // Engine-specific: register additional light classes for editor
        app
            // Light classes (for Properties panel)
            .register_type::<EustressPointLight>()
            .register_type::<EustressSpotLight>()
            .register_type::<SurfaceLight>()
            .register_type::<LightingServiceOwner>()
            
            // Celestial classes
            .register_type::<SunClass>()
            .register_type::<MoonClass>()
            
            // Environment classes
            .register_type::<Terrain>()
            .register_type::<Atmosphere>()
            
            // Hydrate file-loaded Lighting/ entities with real ECS components.
            // Runs every frame — detects Instance entities with lighting
            // class names that lack their real Bevy components and attaches
            // DirectionalLight, SunMarker, SunClass, MoonMarker, etc.
            // This is the authoritative path for per-Space lighting.
            .add_systems(Update, hydrate_lighting_entities)
            // Sync Sun class properties with LightingService
            .add_systems(Update, sync_sun_with_lighting_service)
            // Update directional light from Sun class (latitude-based positioning)
            .add_systems(Update, update_directional_light_from_sun_class.after(sync_sun_with_lighting_service))
            // Sync Atmosphere entity with SceneAtmosphere resource for rendering
            .add_systems(Update, sync_atmosphere_to_rendering)
            // Sync Lighting ServiceComponent property edits → LightingService resource
            .add_systems(Update, sync_service_properties_to_lighting);
    }
}

/// Hydrate file-loaded Lighting/ entities with real ECS components.
///
/// The file loader spawns `Instance` entities from `Lighting/*.instance.toml`
/// but only attaches generic components (Instance, Transform, Visibility,
/// Attributes, Name). This system detects entities with lighting class names
/// that lack their real Bevy components and attaches:
///
/// - **Star → DirectionalLight + SunMarker + SunClass + cascade shadows + SunDisk**
/// - **Moon → DirectionalLight + MoonMarker + MoonClass**
/// - **Sky → Sky component**
/// - **Atmosphere → Atmosphere + EustressAtmosphere**
///
/// Runs every `Update` frame; cheap no-op when all entities are hydrated
/// (the `Without<>` filter ensures empty query iteration).
fn hydrate_lighting_entities(
    mut commands: Commands,
    lighting: Res<LightingService>,
    // Star entities that have Instance but lack SunMarker (not yet hydrated)
    unhydrated_sun: Query<(Entity, &Instance), (Without<SunMarker>, Without<MoonMarker>)>,
    // Moon entities that have Instance but lack MoonMarker
    unhydrated_moon: Query<(Entity, &Instance), (Without<MoonMarker>, Without<SunMarker>)>,
    // Sky entities that lack Sky component
    unhydrated_sky: Query<(Entity, &Instance), Without<Sky>>,
    // Atmosphere entities that lack EustressAtmosphere component
    unhydrated_atmo: Query<(Entity, &Instance), Without<EustressAtmosphere>>,
) {
    // ── Star → Sun (DirectionalLight + SunMarker + SunClass) ──────────
    for (entity, instance) in unhydrated_sun.iter() {
        if instance.class_name != ClassName::Star { continue; }

        info!("☀️ Hydrating Sun entity {:?} from Lighting/ TOML", entity);

        let sun_class = SunClass {
            enabled: true,
            time_of_day: lighting.time_of_day * 24.0,
            cycle_speed: 0.0,
            cycle_paused: true,
            latitude: lighting.geographic_latitude,
            day_of_year: 172,
            angular_size: lighting.sun_angular_radius * 2.0,
            noon_color: lighting.sun_color,
            horizon_color: [1.0, 0.5, 0.2, 1.0],
            noon_intensity: lighting.sun_intensity,
            horizon_intensity: 1000.0,
            cast_shadows: lighting.shadows_enabled,
            shadow_softness: lighting.shadow_softness,
            ambient_day_color: lighting.ambient,
            ambient_night_color: [0.02, 0.02, 0.05, 1.0],
            corona_intensity: 0.3,
            god_rays_intensity: 0.0,
            texture: String::new(),
        };

        let sun_dir = lighting.sun_direction();
        let cascade_shadow_config = CascadeShadowConfigBuilder {
            num_cascades: 4,
            minimum_distance: 0.1,
            maximum_distance: 2048.0,
            first_cascade_far_bound: 16.0,
            overlap_proportion: 0.3,
            ..default()
        }
        .build();

        commands.entity(entity).insert((
            DirectionalLight {
                color: Color::srgba(
                    lighting.sun_color[0],
                    lighting.sun_color[1],
                    lighting.sun_color[2],
                    lighting.sun_color[3],
                ),
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
            VolumetricLight,
            cascade_shadow_config,
            SunMarker,
            sun_class,
            LightingServiceOwner,
        ));
    }

    // ── Moon → DirectionalLight + MoonMarker + MoonClass ──────────────
    for (entity, instance) in unhydrated_moon.iter() {
        if instance.class_name != ClassName::Moon { continue; }

        info!("🌙 Hydrating Moon entity {:?} from Lighting/ TOML", entity);

        commands.entity(entity).insert((
            DirectionalLight {
                color: Color::srgb(0.7, 0.75, 0.9),
                illuminance: 500.0,
                shadows_enabled: false,
                ..default()
            },
            Transform::from_xyz(50.0, 80.0, -30.0)
                .looking_at(Vec3::ZERO, Vec3::Y),
            MoonMarker,
            MoonClass::default(),
            LightingServiceOwner,
        ));
    }

    // ── Sky → Sky component ───────────────────────────────────────────
    for (entity, instance) in unhydrated_sky.iter() {
        if instance.class_name != ClassName::Sky { continue; }

        info!("�️ Hydrating Sky entity {:?} from Lighting/ TOML", entity);
        commands.entity(entity).insert((
            Sky::default(),
            LightingServiceOwner,
        ));
    }

    // ── Atmosphere → Atmosphere + EustressAtmosphere ──────────────────
    for (entity, instance) in unhydrated_atmo.iter() {
        if instance.class_name != ClassName::Atmosphere { continue; }

        info!("🌫️ Hydrating Atmosphere entity {:?} from Lighting/ TOML", entity);
        commands.entity(entity).insert((
            Atmosphere::clear_day(),
            EustressAtmosphere::default(),
            LightingServiceOwner,
        ));
    }
}

/// Sync Sun class properties with LightingService for real-time updates
/// Geographic latitude from LightingService controls sun/moon arc paths
fn sync_sun_with_lighting_service(
    lighting: Res<LightingService>,
    mut sun_query: Query<&mut SunClass>,
) {
    if !lighting.is_changed() {
        return;
    }
    
    for mut sun in sun_query.iter_mut() {
        // Sync latitude from LightingService (controls sun arc path)
        sun.latitude = lighting.geographic_latitude;
        
        // Parse ClockTime string to time_of_day if it changed
        if let Some((hours, minutes)) = parse_clock_time(&lighting.clock_time) {
            let time = hours as f32 + (minutes as f32 / 60.0);
            if (sun.time_of_day - time).abs() > 0.01 {
                sun.time_of_day = time;
            }
        }
    }
}

/// Parse clock time string "HH:MM:SS" to (hours, minutes)
fn parse_clock_time(clock_time: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = clock_time.split(':').collect();
    if parts.len() >= 2 {
        let hours = parts[0].parse().ok()?;
        let minutes = parts[1].parse().ok()?;
        Some((hours, minutes))
    } else {
        None
    }
}

/// Update directional light position and properties from Sun class
/// Uses latitude-based sun position calculation for realistic sun arcs
fn update_directional_light_from_sun_class(
    sun_class_query: Query<&SunClass, Changed<SunClass>>,
    mut light_query: Query<(&mut DirectionalLight, &mut Transform), With<SunMarker>>,
) {
    for sun in sun_class_query.iter() {
        if !sun.enabled {
            continue;
        }
        
        // Get direction from Sun class (uses latitude, day_of_year, time_of_day)
        let sun_dir = sun.direction();
        let sun_distance = 100.0;
        
        // Get current color and intensity based on elevation
        let color = sun.current_color();
        let intensity = sun.current_intensity();
        
        // Update directional light
        if let Ok((mut light, mut transform)) = light_query.single_mut() {
            light.color = Color::srgba(color[0], color[1], color[2], color[3]);
            light.illuminance = intensity;
            light.shadows_enabled = sun.cast_shadows;
            
            // Position light in direction of sun
            transform.translation = sun_dir * sun_distance;
            transform.look_at(Vec3::ZERO, Vec3::Y);
        }
    }
}

/// Sync Atmosphere entity properties with SceneAtmosphere resource for rendering
/// When the Atmosphere entity in Explorer is modified, update the rendering resource
fn sync_atmosphere_to_rendering(
    atmosphere_query: Query<&Atmosphere, Changed<Atmosphere>>,
    eustress_atmo_query: Query<&EustressAtmosphere, Changed<EustressAtmosphere>>,
    mut scene_atmosphere: ResMut<eustress_common::plugins::lighting_plugin::SceneAtmosphere>,
) {
    // Sync from Atmosphere class component (Explorer entity)
    for atmosphere in atmosphere_query.iter() {
        // Convert Atmosphere class to EustressAtmosphere for rendering
        scene_atmosphere.atmosphere.density = atmosphere.density;
        scene_atmosphere.atmosphere.offset = atmosphere.offset;
        scene_atmosphere.atmosphere.color = atmosphere.color;
        scene_atmosphere.atmosphere.decay = atmosphere.decay;
        scene_atmosphere.atmosphere.glare = atmosphere.glare;
        scene_atmosphere.atmosphere.haze = atmosphere.haze;
        
        info!("🌫️ Synced Atmosphere to rendering (density: {}, haze: {})", 
              atmosphere.density, atmosphere.haze);
    }
    
    // Also sync from EustressAtmosphere if it was modified directly
    for eustress_atmo in eustress_atmo_query.iter() {
        scene_atmosphere.atmosphere = eustress_atmo.clone();
        info!("🌫️ Synced EustressAtmosphere to rendering");
    }
}

/// Sync Lighting ServiceComponent property edits → LightingService resource.
///
/// When the user edits ClockTime, Brightness, etc. in the Properties panel,
/// those changes go to ServiceComponent first. This system reads them and
/// writes to the live LightingService resource so Bevy systems react immediately.
fn sync_service_properties_to_lighting(
    mut lighting: ResMut<LightingService>,
    service_query: Query<&crate::space::service_loader::ServiceComponent, Changed<crate::space::service_loader::ServiceComponent>>,
) {
    use crate::space::service_loader::PropertyValue;

    for service in service_query.iter() {
        // Only sync the Lighting service
        if service.class_name != "Lighting" { continue; }

        let props = &service.properties;

        if let Some(PropertyValue::Float(v)) = props.get("clock_time") {
            let new_tod = (*v as f32) / 24.0; // ClockTime is hours (0-24), time_of_day is 0-1
            if (lighting.time_of_day - new_tod).abs() > 0.001 {
                lighting.time_of_day = new_tod.clamp(0.0, 1.0);
                let hours = (lighting.time_of_day * 24.0) as u32;
                let minutes = ((lighting.time_of_day * 24.0 * 60.0) % 60.0) as u32;
                lighting.clock_time = format!("{:02}:{:02}:00", hours, minutes);
            }
        }
        if let Some(PropertyValue::Float(v)) = props.get("brightness") {
            lighting.brightness = *v as f32;
        }
        if let Some(PropertyValue::Float(v)) = props.get("shadow_softness") {
            lighting.shadow_softness = *v as f32;
        }
        if let Some(PropertyValue::Float(v)) = props.get("exposure_compensation") {
            lighting.exposure_compensation = *v as f32;
        }
        if let Some(PropertyValue::Float(v)) = props.get("geographic_latitude") {
            lighting.geographic_latitude = *v as f32;
        }
        if let Some(PropertyValue::Float(v)) = props.get("fog_start") {
            lighting.fog_start = *v as f32;
        }
        if let Some(PropertyValue::Float(v)) = props.get("fog_end") {
            lighting.fog_end = *v as f32;
        }
        if let Some(PropertyValue::Bool(v)) = props.get("fog_enabled") {
            lighting.fog_enabled = *v;
        }
        if let Some(PropertyValue::Bool(v)) = props.get("shadows_enabled") {
            lighting.shadows_enabled = *v;
        }
        if let Some(PropertyValue::Float(v)) = props.get("sun_intensity") {
            lighting.sun_intensity = *v as f32;
        }
        if let Some(PropertyValue::Float(v)) = props.get("environment_diffuse_scale") {
            lighting.environment_diffuse_scale = *v as f32;
        }
        if let Some(PropertyValue::Float(v)) = props.get("environment_specular_scale") {
            lighting.environment_specular_scale = *v as f32;
        }
        if let Some(PropertyValue::Bool(v)) = props.get("cycle_enabled") {
            lighting.cycle_enabled = *v;
        }
        if let Some(PropertyValue::Float(v)) = props.get("day_length_minutes") {
            lighting.day_length_minutes = *v as f32;
        }
        // Color arrays (stored as Vec4 in ServiceComponent)
        if let Some(PropertyValue::Vec4(v)) = props.get("fog_color") {
            lighting.fog_color = [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
        }
        if let Some(PropertyValue::Vec4(v)) = props.get("ambient") {
            lighting.ambient = [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
        }
        if let Some(PropertyValue::Vec4(v)) = props.get("outdoor_ambient") {
            lighting.outdoor_ambient = [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
        }
        if let Some(PropertyValue::Vec4(v)) = props.get("sun_color") {
            lighting.sun_color = [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
        }
        if let Some(PropertyValue::Vec4(v)) = props.get("sky_color") {
            lighting.sky_color = [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
        }
        if let Some(PropertyValue::Vec4(v)) = props.get("horizon_color") {
            lighting.horizon_color = [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
        }
    }
}
