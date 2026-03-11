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
        app.add_systems(Update, diagnose_scene_once.run_if(bevy::time::common_conditions::once_after_real_delay(std::time::Duration::from_secs(6))));
    }
}

/// One-shot diagnostic: dump all Camera3d and Mesh3d entities after 3 seconds
fn diagnose_scene_once(
    cameras: Query<(Entity, &Transform, &Camera), With<Camera3d>>,
    meshes: Query<(Entity, &Transform, Option<&Name>), With<Mesh3d>>,
    scene_roots: Query<(Entity, &SceneRoot, Option<&Name>, &Transform)>,
    instances: Query<(Entity, &eustress_common::classes::Instance)>,
    children_query: Query<&Children>,
    all_entities: Query<(Entity, Option<&Name>)>,
    asset_server: Res<AssetServer>,
) {
    info!("=== SCENE DIAGNOSTIC (3s after startup) ===");
    info!("Total entities: {}", all_entities.iter().count());
    info!("Instance entities: {}", instances.iter().count());
    info!("SceneRoot entities: {}", scene_roots.iter().count());
    for (entity, scene_root, name, transform) in scene_roots.iter().take(5) {
        let child_count = children_query.get(entity).map(|c| c.len()).unwrap_or(0);
        let load_state = asset_server.get_load_state(scene_root.0.id());
        let dep_load_state = asset_server.get_recursive_dependency_load_state(scene_root.0.id());
        let asset_path = asset_server.get_path(scene_root.0.id());
        info!("  SceneRoot {:?} '{}': pos={}, children={}, load_state={:?}, dep_state={:?}, path={:?}",
            entity, name.map(|n| n.as_str()).unwrap_or("unnamed"), transform.translation, child_count, load_state, dep_load_state, asset_path);
    }
    info!("Camera3d entities: {}", cameras.iter().count());
    for (entity, transform, camera) in cameras.iter() {
        info!("  Camera {:?}: pos={} order={} viewport={:?}",
            entity, transform.translation, camera.order, camera.viewport);
    }
    info!("Mesh3d entities: {}", meshes.iter().count());
    for (entity, transform, name) in meshes.iter() {
        info!("  Mesh {:?} '{}': pos={}",
            entity, name.map(|n| n.as_str()).unwrap_or("unnamed"), transform.translation);
    }
    info!("=== END SCENE DIAGNOSTIC ===");
}

pub fn setup_default_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    skybox_handle: Res<SkyboxHandle>,
    startup_args: Res<StartupArgs>,
    asset_server: Res<AssetServer>,
) {
    // Check if we're loading a scene file - if so, skip default content
    let loading_scene_file = startup_args.scene_file.is_some();
    
    if loading_scene_file {
        println!("🎬 Scene file specified - skipping default scene content...");
    } else {
        println!("🎬 Setting up default scene (shared with Client)...");
    }
    
    // =========================================================================
    // CAMERA - Editor camera (dark background like egui era)
    // Always spawn the camera regardless of scene file
    // =========================================================================
    
    // Spawn camera — skybox will be auto-attached by SharedLightingPlugin's
    // attach_skybox_to_cameras system (same as egui era)
    commands.spawn((
        Camera3d::default(),
        Tonemapping::Reinhard,
        Transform::from_xyz(10.0, 8.0, 10.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        Projection::Perspective(PerspectiveProjection {
            fov: 70.0_f32.to_radians(),
            near: 0.1,
            far: 10000.0,
            ..default()
        }),
        Instance {
            name: "Camera".to_string(),
            class_name: ClassName::Camera,
            archivable: true,
            id: 0,
            ..Default::default()
        },
        Name::new("Camera"),
    ));
    
    // =========================================================================
    // SPAWN DEFAULT SCENE - Only if NOT loading a scene file
    // =========================================================================
    
    // NOTE: Instance loading is handled by SpaceFileLoaderPlugin (file_loader.rs)
    // which properly creates folder hierarchy with parent-child relationships.
    // Do NOT load instances here to avoid duplicates.
    if !loading_scene_file {
        println!("✅ Default scene ready — instances loaded by SpaceFileLoaderPlugin");
    } else {
        println!("⏭️ Skipping default scene content (loading scene file)");
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
            ..Default::default()
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
            ..Default::default()
        },
        Atmosphere::clear_day(),
        Name::new("Atmosphere"),
    ));
    
    // Note: Sun and Moon entities are spawned by SharedLightingPlugin with DirectionalLight
    // They include both marker components and class components for full functionality
    
    println!("🌤️ Sky and Atmosphere entities spawned for Lighting service");
    println!("☀️ Sun and Moon are spawned by SharedLightingPlugin with DirectionalLight");
}

/// Grid rendering system - 9.80665m tessellation (SI standard gravity)
pub fn draw_grid(mut gizmos: Gizmos) {
    let grid_size = 20;
    let grid_spacing = 9.80665 / 10.0; // 9.80665 / 10 = 0.980665 per cell
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
// Instance loading is handled by SpaceFileLoaderPlugin (file_loader.rs)
