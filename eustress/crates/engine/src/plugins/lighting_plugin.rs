//! # Lighting Plugin
//! 
//! Uses the shared lighting plugin from eustress_common.
//! Adds engine-specific light class registrations.
//! Spawns Sun, Moon, and Atmosphere as proper Explorer entities under Lighting service.
//! 
//! ## Default Lighting Children
//! - Sun: Controls day/night cycle, directional lighting based on ClockTime and GeographicLatitude
//! - Moon: Night lighting with phases
//! - Atmosphere: Post-processing atmospheric effects (haze, fog, sky color)

use bevy::prelude::*;
use eustress_common::classes::{
    ClassName, Instance, EustressPointLight, EustressSpotLight, SurfaceLight, Terrain, Atmosphere,
    Sun as SunClass, Moon as MoonClass,
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
            
            // Add system to insert Sun, Moon, Atmosphere as Explorer entities
            .add_systems(PostStartup, setup_lighting_explorer_entities)
            // Sync Sun class properties with LightingService
            .add_systems(Update, sync_sun_with_lighting_service)
            // Update directional light from Sun class (latitude-based positioning)
            .add_systems(Update, update_directional_light_from_sun_class.after(sync_sun_with_lighting_service))
            // Sync Atmosphere entity with SceneAtmosphere resource for rendering
            .add_systems(Update, sync_atmosphere_to_rendering);
    }
}

/// Setup Sun, Moon, and Atmosphere as proper Explorer entities under Lighting service
/// This runs after SharedLightingPlugin's setup_lighting to add Instance and class components
fn setup_lighting_explorer_entities(
    mut commands: Commands,
    sun_query: Query<Entity, (With<SunMarker>, Without<Instance>)>,
    moon_query: Query<Entity, (With<MoonMarker>, Without<Instance>)>,
    atmosphere_query: Query<Entity, (With<EustressAtmosphere>, With<Instance>)>,
    lighting: Res<LightingService>,
) {
    // Add Instance and SunClass to Sun entity so it appears in Explorer under Lighting
    for entity in sun_query.iter() {
        info!("☀️ Adding Explorer components to Sun entity");
        
        // Create Sun class with properties from LightingService
        let sun_class = SunClass {
            enabled: true,
            time_of_day: lighting.time_of_day * 24.0, // Convert 0-1 to 0-24
            cycle_speed: 0.0,
            cycle_paused: true,
            latitude: lighting.geographic_latitude,
            day_of_year: 172, // Summer solstice
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
        
        commands.entity(entity).insert((
            Instance {
                name: "Sun".to_string(),
                class_name: ClassName::Sun,
                archivable: true,
                id: entity.index().index(),
                ..Default::default()
            },
            sun_class,
            LightingServiceOwner,
        ));
    }
    
    // Add Instance and MoonClass to Moon entity if it exists
    for entity in moon_query.iter() {
        info!("🌙 Adding Explorer components to Moon entity");
        commands.entity(entity).insert((
            Instance {
                name: "Moon".to_string(),
                class_name: ClassName::Moon,
                archivable: true,
                id: entity.index().index(),
                ..Default::default()
            },
            MoonClass::default(),
            LightingServiceOwner,
        ));
    }
    
    // NOTE: Moon is already spawned by SharedLightingPlugin in eustress_common
    // Do NOT spawn another one here - that causes duplicate moons!
    
    // If no Atmosphere exists, spawn one as default child of Lighting
    if atmosphere_query.is_empty() {
        info!("🌫️ Spawning Atmosphere entity for Lighting service");
        let atmo_entity = commands.spawn((
            Atmosphere::default(),
            EustressAtmosphere::default(),
            Name::new("Atmosphere"),
            LightingServiceOwner,
            Instance {
                name: "Atmosphere".to_string(),
                class_name: ClassName::Atmosphere,
                archivable: true,
                id: 0,
                ..Default::default()
            },
        )).id();
        
        // Update instance ID
        commands.entity(atmo_entity).insert(Instance {
            name: "Atmosphere".to_string(),
            class_name: ClassName::Atmosphere,
            archivable: true,
            id: atmo_entity.index().index(),
            ..Default::default()
        });
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
