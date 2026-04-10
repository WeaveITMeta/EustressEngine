// ============================================================================
// Eustress Engine - Analytical Sun/Moon Disc Shader
//
// Resolution-independent celestial disc rendering via billboard quad +
// custom fragment shader. Replaces the pixelated cubemap-baked discs.
//
// Architecture:
// - SunDiscMaterial: ExtendedMaterial with custom fragment for smooth disc
// - Billboard quad spawned at sun/moon position, always faces camera
// - sync_sun_disc_transform: tracks SunClass direction each frame
// ============================================================================

use bevy::prelude::*;
use bevy::pbr::{MaterialExtension, ExtendedMaterial, OpaqueRendererMethod};
use bevy::render::render_resource::{AsBindGroup, ShaderRef};

/// Marker for the analytical sun disc entity
#[derive(Component)]
pub struct SunDiscMarker;

/// Marker for the analytical moon disc entity
#[derive(Component)]
pub struct MoonDiscMarker;

/// Custom material for the sun/moon disc fragment shader
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct SunDiscMaterial {
    #[uniform(0)]
    pub color: LinearRgba,
    #[uniform(0)]
    pub corona_color: LinearRgba,
    #[uniform(0)]
    pub disc_radius: f32,
    #[uniform(0)]
    pub corona_radius: f32,
    #[uniform(0)]
    pub intensity: f32,
    #[uniform(0)]
    pub _padding: f32,
}

impl MaterialExtension for SunDiscMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/sun_disc.wgsl".into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        "shaders/sun_disc.wgsl".into()
    }
}

type SunMaterial = ExtendedMaterial<StandardMaterial, SunDiscMaterial>;

pub struct SunDiscPlugin;

impl Plugin for SunDiscPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<SunMaterial>::default())
            .add_systems(Startup, spawn_sun_disc)
            .add_systems(Update, sync_sun_disc_transforms);
    }
}

/// Spawn billboard quads for sun and moon discs
fn spawn_sun_disc(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut sun_materials: ResMut<Assets<SunMaterial>>,
) {
    // Quad mesh for billboard — will be oriented to face camera each frame
    let quad = meshes.add(Rectangle::new(1.0, 1.0));

    // Sun disc material
    let sun_mat = sun_materials.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        },
        extension: SunDiscMaterial {
            color: LinearRgba::new(1.0, 0.98, 0.92, 1.0),
            corona_color: LinearRgba::new(1.0, 0.9, 0.7, 1.0),
            disc_radius: 0.3,   // Disc fills 30% of the quad
            corona_radius: 0.8, // Corona extends to 80%
            intensity: 2.0,     // HDR brightness
            _padding: 0.0,
        },
    });

    // Sun billboard — positioned by sync_sun_disc_transforms
    commands.spawn((
        Mesh3d(quad.clone()),
        MeshMaterial3d(sun_mat),
        Transform::from_translation(Vec3::new(0.0, 80.0, 0.0))
            .with_scale(Vec3::splat(12.0)), // Billboard size in world units
        SunDiscMarker,
        NotShadowCaster,
        Name::new("SunDisc"),
    ));

    // Moon disc material — cooler, dimmer
    let moon_mat = sun_materials.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        },
        extension: SunDiscMaterial {
            color: LinearRgba::new(0.85, 0.87, 0.92, 1.0),
            corona_color: LinearRgba::new(0.7, 0.75, 0.85, 1.0),
            disc_radius: 0.35,
            corona_radius: 0.7,
            intensity: 0.8,
            _padding: 0.0,
        },
    });

    // Moon billboard
    commands.spawn((
        Mesh3d(quad),
        MeshMaterial3d(moon_mat),
        Transform::from_translation(Vec3::new(0.0, -80.0, 0.0))
            .with_scale(Vec3::splat(8.0)),
        MoonDiscMarker,
        NotShadowCaster,
        Name::new("MoonDisc"),
    ));

    info!("☀ Analytical sun/moon disc shaders spawned");
}

/// Every frame: position sun/moon disc billboards at the correct sky position
/// and orient them to face the camera.
fn sync_sun_disc_transforms(
    camera_query: Query<&GlobalTransform, (With<Camera3d>, Without<SunDiscMarker>, Without<MoonDiscMarker>)>,
    sun_class: Query<&eustress_common::classes::SunClass, With<eustress_common::plugins::lighting_plugin::SunMarker>>,
    mut sun_disc: Query<&mut Transform, (With<SunDiscMarker>, Without<MoonDiscMarker>)>,
    mut moon_disc: Query<&mut Transform, (With<MoonDiscMarker>, Without<SunDiscMarker>)>,
) {
    let Some(camera_gt) = camera_query.iter().find(|_| true) else { return };
    let cam_pos = camera_gt.translation();

    // Get sun direction from SunClass (same source as skybox)
    let sun_dir = sun_class.iter().next()
        .map(|sc| sc.direction())
        .unwrap_or(Vec3::new(0.3, 0.8, 0.2).normalize());

    let sky_distance = 500.0; // Far enough to be behind all geometry

    // Sun disc — position along sun direction, face camera
    if let Ok(mut sun_t) = sun_disc.single_mut() {
        sun_t.translation = cam_pos + sun_dir * sky_distance;
        sun_t.look_at(cam_pos, Vec3::Y);
        // Hide when sun is below horizon
        // (scale to zero rather than visibility toggle to avoid Bevy visibility propagation cost)
        if sun_dir.y < -0.02 {
            sun_t.scale = Vec3::ZERO;
        } else {
            sun_t.scale = Vec3::splat(12.0);
        }
    }

    // Moon disc — simplified opposite of sun
    let moon_dir = (-sun_dir).normalize();
    if let Ok(mut moon_t) = moon_disc.single_mut() {
        moon_t.translation = cam_pos + moon_dir * sky_distance;
        moon_t.look_at(cam_pos, Vec3::Y);
        if moon_dir.y < -0.02 {
            moon_t.scale = Vec3::ZERO;
        } else {
            moon_t.scale = Vec3::splat(8.0);
        }
    }
}
