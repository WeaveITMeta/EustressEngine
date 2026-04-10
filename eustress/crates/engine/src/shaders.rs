// ============================================================================
// Eustress Engine - Sun/Moon Disc Rendering
//
// Uses StandardMaterial (unlit, emissive) billboard quads.
// No custom WGSL — guaranteed to render on all GPU backends.
// ============================================================================

use bevy::prelude::*;

/// Marker for the sun disc entity
#[derive(Component)]
pub struct SunDiscMarker;

/// Marker for the moon disc entity
#[derive(Component)]
pub struct MoonDiscMarker;

pub struct SunDiscPlugin;

impl Plugin for SunDiscPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_sun_disc)
            .add_systems(Update, sync_sun_disc_transforms);
    }
}

fn spawn_sun_disc(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let quad = meshes.add(Circle::new(0.5)); // Circle mesh for round disc

    // Sun — bright emissive white-yellow, unlit
    let sun_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.98, 0.9),
        emissive: LinearRgba::new(5.0, 4.9, 4.0, 1.0),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });

    commands.spawn((
        Mesh3d(quad.clone()),
        MeshMaterial3d(sun_mat),
        Transform::from_translation(Vec3::new(0.0, 80.0, 0.0))
            .with_scale(Vec3::splat(40.0)),
        SunDiscMarker,
        bevy::light::NotShadowCaster,
        Name::new("SunDisc"),
    ));

    // Moon — pale silver, dimmer
    let moon_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.85, 0.87, 0.92, 0.9),
        emissive: LinearRgba::new(0.8, 0.82, 0.88, 1.0),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });

    commands.spawn((
        Mesh3d(quad),
        MeshMaterial3d(moon_mat),
        Transform::from_translation(Vec3::new(0.0, -80.0, 0.0))
            .with_scale(Vec3::splat(25.0)),
        MoonDiscMarker,
        bevy::light::NotShadowCaster,
        Name::new("MoonDisc"),
    ));

    info!("☀ Sun/Moon disc billboards spawned");
}

fn sync_sun_disc_transforms(
    camera_query: Query<&GlobalTransform, (With<Camera3d>, Without<SunDiscMarker>, Without<MoonDiscMarker>)>,
    sun_class: Query<&eustress_common::classes::Sun, With<eustress_common::services::lighting::Sun>>,
    moon_class: Query<&eustress_common::classes::Moon, With<eustress_common::services::lighting::Moon>>,
    mut sun_disc: Query<&mut Transform, (With<SunDiscMarker>, Without<MoonDiscMarker>)>,
    mut moon_disc: Query<&mut Transform, (With<MoonDiscMarker>, Without<SunDiscMarker>)>,
) {
    let Some(camera_gt) = camera_query.iter().next() else { return };
    let cam_pos = camera_gt.translation();

    let sun_data = sun_class.iter().next();
    let sun_dir = sun_data
        .map(|sc| sc.direction())
        .unwrap_or(Vec3::new(0.3, 0.8, 0.2).normalize());

    let sky_distance = 500.0;

    // Sun disc — billboard always faces camera
    if let Ok(mut sun_t) = sun_disc.single_mut() {
        sun_t.translation = cam_pos + sun_dir * sky_distance;
        sun_t.look_at(cam_pos, Vec3::Y);
        sun_t.scale = if sun_dir.y < -0.02 { Vec3::ZERO } else { Vec3::splat(40.0) };
    }

    // Moon disc — realistic orbital position
    let moon_data = moon_class.iter().next();
    let moon_dir = match (moon_data, sun_data) {
        (Some(moon), Some(sun)) => moon.direction_realistic(sun),
        _ => (-sun_dir).normalize(),
    };
    if let Ok(mut moon_t) = moon_disc.single_mut() {
        moon_t.translation = cam_pos + moon_dir * sky_distance;
        moon_t.look_at(cam_pos, Vec3::Y);
        moon_t.scale = if moon_dir.y < -0.02 { Vec3::ZERO } else { Vec3::splat(25.0) };
    }
}
