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
        app.add_systems(Update, diagnose_scene_once.run_if(bevy::time::common_conditions::once_after_real_delay(std::time::Duration::from_secs(3))));
    }
}

/// One-shot diagnostic: dump all Camera3d and Mesh3d entities after 3 seconds
fn diagnose_scene_once(
    cameras: Query<(Entity, &Transform, &Camera), With<Camera3d>>,
    meshes: Query<(Entity, &Transform, &Name), With<Mesh3d>>,
    all_entities: Query<(Entity, Option<&Name>)>,
) {
    info!("=== SCENE DIAGNOSTIC (3s after startup) ===");
    info!("Total entities: {}", all_entities.iter().count());
    info!("Camera3d entities: {}", cameras.iter().count());
    for (entity, transform, camera) in cameras.iter() {
        info!("  Camera {:?}: pos={} order={} viewport={:?}",
            entity, transform.translation, camera.order, camera.viewport);
    }
    info!("Mesh3d entities: {}", meshes.iter().count());
    for (entity, transform, name) in meshes.iter() {
        info!("  Mesh {:?} '{}': pos={}",
            entity, name, transform.translation);
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
    
    if !loading_scene_file {
        // FILE-SYSTEM-FIRST: Load all default parts from Universe1/spaces/Space1/Workspace
        let universe_path = std::path::PathBuf::from("C:/Users/miksu/Documents/Eustress/Universe1");
        let workspace_path = universe_path.join("spaces/Space1/Workspace");
        
        // Load Baseplate from .glb file
        let baseplate_path = workspace_path.join("Baseplate.glb");
        if baseplate_path.exists() {
            let baseplate_scene = asset_server.load(format!("{}#Scene0", baseplate_path.display()));
            let baseplate_entity = commands.spawn((
                SceneRoot(baseplate_scene),
                Transform::from_xyz(0.0, -0.5, 0.0),
                eustress_common::classes::Instance {
                    name: "Baseplate".to_string(),
                    class_name: eustress_common::classes::ClassName::Part,
                    archivable: true,
                    id: 1,
                    ai: false,
                },
                eustress_common::default_scene::PartEntityMarker {
                    part_id: "Baseplate".to_string(),
                },
                Name::new("Baseplate"),
            )).id();
            println!("🟫 Loaded Baseplate from .glb file: {:?}", baseplate_entity);
        } else {
            // Fallback: spawn programmatically if .glb doesn't exist
            let baseplate_entity = eustress_common::spawn_baseplate(&mut commands, &mut meshes, &mut materials);
            println!("⚠️ Baseplate .glb not found, spawned programmatically: {:?}", baseplate_entity);
        }
        
        // Load Welcome Cube from .glb file
        let welcome_cube_path = workspace_path.join("Welcome Cube.glb");
        if welcome_cube_path.exists() {
            let cube_scene = asset_server.load(format!("{}#Scene0", welcome_cube_path.display()));
            let cube_entity = commands.spawn((
                SceneRoot(cube_scene),
                Transform::from_xyz(0.0, 0.980665, 0.0),
                eustress_common::classes::Instance {
                    name: "Welcome Cube".to_string(),
                    class_name: eustress_common::classes::ClassName::Part,
                    archivable: true,
                    id: 2,
                    ai: false,
                },
                eustress_common::default_scene::PartEntityMarker {
                    part_id: "Welcome Cube".to_string(),
                },
                Name::new("Welcome Cube"),
            )).id();
            println!("🟩 Loaded Welcome Cube from .glb file: {:?}", cube_entity);
        } else {
            // Fallback: spawn programmatically if .glb doesn't exist
            let cube_entity = eustress_common::spawn_welcome_cube(&mut commands, &mut meshes, &mut materials);
            println!("⚠️ Welcome Cube .glb not found, spawned programmatically: {:?}", cube_entity);
        }
        
        println!("✅ Default scene ready (file-system-first: Baseplate + Welcome Cube from Universe1/spaces/Space1)!");
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
