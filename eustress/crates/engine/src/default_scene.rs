use bevy::prelude::*;
use bevy::core_pipeline::tonemapping::Tonemapping;
use eustress_common::plugins::lighting_plugin::SkyboxHandle;
use eustress_common::classes::{Instance, ClassName, Sky, Atmosphere};
use crate::startup::StartupArgs;

/// Plugin to set up the default scene with camera and ground
/// Lighting is handled by SharedLightingPlugin (same as client)
pub struct DefaultScenePlugin;

impl Plugin for DefaultScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_default_scene);
    }
}

pub fn setup_default_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    skybox_handle: Res<SkyboxHandle>,
    startup_args: Res<StartupArgs>,
) {
    // Check if we're loading a scene file - if so, skip default content
    let loading_scene_file = startup_args.scene_file.is_some();
    
    if loading_scene_file {
        println!("🎬 Scene file specified - skipping default scene content...");
    } else {
        println!("🎬 Setting up default scene (shared with Client)...");
    }
    
    // =========================================================================
    // CAMERA - Editor camera with skybox from shared lighting plugin
    // Always spawn the camera regardless of scene file
    // =========================================================================
    
    // Get skybox handle from shared lighting plugin
    if let Some(ref skybox_image) = skybox_handle.handle {
        commands.spawn((
            Camera3d::default(),
            Tonemapping::Reinhard,
            Transform::from_xyz(10.0, 8.0, 10.0)
                .looking_at(Vec3::ZERO, Vec3::Y),
            Projection::Perspective(PerspectiveProjection {
                fov: 70.0_f32.to_radians(),
                ..default()
            }),
            bevy::core_pipeline::Skybox {
                image: skybox_image.clone(),
                brightness: 1000.0,
                ..default()
            },
            EnvironmentMapLight {
                diffuse_map: skybox_image.clone(),
                specular_map: skybox_image.clone(),
                intensity: 400.0,
                ..default()
            },
            // Instance component so Camera appears in Explorer under Workspace
            Instance {
                name: "Camera".to_string(),
                class_name: ClassName::Camera,
                archivable: true,
                id: 0,
            },
            Name::new("Camera"),
        ));
    } else {
        // Fallback camera without skybox
        commands.spawn((
            Camera3d::default(),
            Tonemapping::Reinhard,
            Transform::from_xyz(10.0, 8.0, 10.0)
                .looking_at(Vec3::ZERO, Vec3::Y),
            Projection::Perspective(PerspectiveProjection {
                fov: 70.0_f32.to_radians(),
                ..default()
            }),
            // Instance component so Camera appears in Explorer under Workspace
            Instance {
                name: "Camera".to_string(),
                class_name: ClassName::Camera,
                archivable: true,
                id: 0,
            },
            Name::new("Camera"),
        ));
    }
    
    // =========================================================================
    // SPAWN DEFAULT SCENE - Only if NOT loading a scene file
    // =========================================================================
    
    if !loading_scene_file {
        // Spawn baseplate (512x1x512 dark gray) - shared with client
        eustress_common::spawn_baseplate(&mut commands, &mut meshes, &mut materials);
        
        // Spawn welcome cube (2x2x2 green) - shared with client
        eustress_common::spawn_welcome_cube(&mut commands, &mut meshes, &mut materials);
        
        println!("✅ Default scene ready (shared with Client: Baseplate + Welcome Cube)!");
    }
    
    // =========================================================================
    // LIGHTING ENTITIES - Sky and Atmosphere (always spawn for Explorer)
    // =========================================================================
    
    // Spawn Sky entity (appears under Lighting in Explorer)
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        Instance {
            name: "Sky".to_string(),
            class_name: ClassName::Sky,
            archivable: true,
            id: 0,
        },
        Sky::default(),
        Name::new("Sky"),
    ));
    
    // Spawn Atmosphere entity (appears under Lighting in Explorer)
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        Instance {
            name: "Atmosphere".to_string(),
            class_name: ClassName::Atmosphere,
            archivable: true,
            id: 0,
        },
        Atmosphere::clear_day(),
        Name::new("Atmosphere"),
    ));
    
    // Note: Sun and Moon entities are spawned by SharedLightingPlugin with DirectionalLight
    // They include both marker components and class components for full functionality
    
    println!("🌤️ Sky and Atmosphere entities spawned for Lighting service");
    println!("☀️ Sun and Moon are spawned by SharedLightingPlugin with DirectionalLight");
}

/// Grid rendering system - 9.8x9.8 grid spacing
pub fn draw_grid(mut gizmos: Gizmos) {
    let grid_size = 20;
    let grid_spacing = 0.98; // 9.8 / 10 = 0.98 per cell
    let color_major = Color::srgba(0.3, 0.3, 0.3, 0.8);
    let color_minor = Color::srgba(0.2, 0.2, 0.2, 0.5);
    
    for i in -grid_size..=grid_size {
        let pos = i as f32 * grid_spacing;
        let color = if i % 5 == 0 { color_major } else { color_minor };
        
        // Lines along X axis
        gizmos.line(
            Vec3::new(-grid_size as f32 * grid_spacing, 0.0, pos),
            Vec3::new(grid_size as f32 * grid_spacing, 0.0, pos),
            color,
        );
        
        // Lines along Z axis
        gizmos.line(
            Vec3::new(pos, 0.0, -grid_size as f32 * grid_spacing),
            Vec3::new(pos, 0.0, grid_size as f32 * grid_spacing),
            color,
        );
    }
    
    // Draw origin axes
    gizmos.line(Vec3::ZERO, Vec3::new(3.0, 0.0, 0.0), Color::srgb(1.0, 0.0, 0.0)); // X - Red
    gizmos.line(Vec3::ZERO, Vec3::new(0.0, 3.0, 0.0), Color::srgb(0.0, 1.0, 0.0)); // Y - Green
    gizmos.line(Vec3::ZERO, Vec3::new(0.0, 0.0, 3.0), Color::srgb(0.0, 0.0, 1.0)); // Z - Blue
}

// Skybox is now created by SharedLightingPlugin from eustress_common
