//! # Solar System Scale Example with Hybrid Coordinates
//!
//! Demonstrates automatic Vec3/DVec3 precision switching for planetary-scale scenes.
//!
//! Run with: cargo run --example solar_system_hybrid

use bevy::prelude::*;
use eustress_common::orbital::hybrid_coords::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(HybridCoordsPlugin)
        .add_systems(Startup, setup_solar_system)
        .add_systems(Update, (
            rotate_camera,
            display_distances,
        ))
        .run();
}

fn setup_solar_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Camera at Earth's position (1 AU from Sun)
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 50_000_000.0, 100_000_000.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        HybridPosition::from_dvec3(DVec3::new(AU, 0.0, 50_000_000.0)),
        HybridFocus, // This is the reference point
    ));

    // Sun at origin
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(696_000_000.0))), // Sun radius
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.9, 0.3),
            emissive: LinearRgba::new(10.0, 9.0, 3.0, 1.0),
            ..default()
        })),
        Transform::default(),
        HybridPosition::from_dvec3(DVec3::ZERO),
        SolarBody::SUN,
        Name::new("Sun"),
    ));

    // Earth at 1 AU
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(6_371_000.0))), // Earth radius
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.4, 0.8),
            ..default()
        })),
        Transform::default(),
        HybridPosition::from_dvec3(DVec3::new(AU, 0.0, 0.0)),
        HybridVelocity::from_dvec3(DVec3::new(0.0, 29_780.0, 0.0)), // 29.78 km/s
        SolarBody::EARTH,
        Name::new("Earth"),
    ));

    // Mars at 1.524 AU
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(3_390_000.0))), // Mars radius
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.3, 0.2),
            ..default()
        })),
        Transform::default(),
        HybridPosition::from_dvec3(DVec3::new(1.524 * AU, 0.0, 0.0)),
        HybridVelocity::from_dvec3(DVec3::new(0.0, 24_070.0, 0.0)), // 24.07 km/s
        SolarBody::MARS,
        Name::new("Mars"),
    ));

    // Moon orbiting Earth
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(1_737_000.0))), // Moon radius
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.7, 0.7, 0.7),
            ..default()
        })),
        Transform::default(),
        HybridPosition::from_dvec3(DVec3::new(AU + 384_400_000.0, 0.0, 0.0)),
        HybridVelocity::from_dvec3(DVec3::new(0.0, 29_780.0 + 1_022.0, 0.0)),
        SolarBody::MOON,
        Name::new("Moon"),
    ));

    // Directional light (sunlight)
    commands.spawn((
        DirectionalLight {
            illuminance: 100_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(0.0, 1.0, 0.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // UI text
    commands.spawn((
        Text::new("Solar System - Hybrid Coordinates\n\
                   Camera uses Vec3/DVec3 automatically\n\
                   Press SPACE to toggle info"),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Px(10.0),
            ..default()
        },
        TextColor(Color::WHITE),
    ));
}

fn rotate_camera(
    time: Res<Time>,
    mut camera_query: Query<(&mut Transform, &mut HybridPosition), With<Camera3d>>,
) {
    for (mut transform, mut hybrid_pos) in camera_query.iter_mut() {
        // Rotate camera around Sun
        let angle = time.elapsed_secs_f64() * 0.1;
        let radius = AU;
        
        // Update absolute position (DVec3 - high precision)
        hybrid_pos.absolute.x = radius * angle.cos();
        hybrid_pos.absolute.z = radius * angle.sin();
        
        // Transform is automatically updated by sync_hybrid_to_transform system
        transform.look_at(Vec3::ZERO, Vec3::Y);
    }
}

fn display_distances(
    focus: Res<FocusPosition>,
    bodies: Query<(&HybridPosition, &Name, &SolarBody)>,
) {
    // Print distances every 2 seconds
    static mut LAST_PRINT: f64 = 0.0;
    let now = unsafe { 
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        if t - LAST_PRINT > 2.0 {
            LAST_PRINT = t;
            true
        } else {
            false
        }
    };
    
    if !now {
        return;
    }
    
    println!("\n=== Distances from Camera ===");
    for (pos, name, body) in bodies.iter() {
        let distance = pos.distance_to(&focus.position);
        let precision_mode = if pos.use_high_precision { "DVec3" } else { "Vec3" };
        
        println!(
            "{:10} | {} | {} | Surface gravity: {:.2} m/s²",
            name.as_str(),
            format_distance(distance),
            precision_mode,
            body.surface_gravity()
        );
    }
}
