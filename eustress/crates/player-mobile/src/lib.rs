// =============================================================================
// Eustress Player Mobile - Library Entry Point
// =============================================================================
// Minimal Bevy 0.17 mobile app for Android/iOS
// =============================================================================

use bevy::prelude::*;

// =============================================================================
// Main Entry Point (works for Android via bevy_main attribute)
// =============================================================================

#[bevy_main]
fn main() {
    #[cfg(all(target_os = "android", feature = "android"))]
    {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("EustressPlayer"),
        );
    }
    
    run_game();
}

/// Shared game runner
pub fn run_game() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Eustress Player".to_string(),
                resizable: false,
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup_mobile)
        .run();
}

/// Initial mobile setup.
fn setup_mobile(mut commands: Commands) {
    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 5.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    
    // Simple cube to verify rendering
    commands.spawn((
        Mesh3d(bevy::asset::Handle::default()),
        MeshMaterial3d::<StandardMaterial>(bevy::asset::Handle::default()),
        Transform::from_xyz(0.0, 0.5, 0.0),
    ));
    
    // Light
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
