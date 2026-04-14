// ============================================================================
// Eustress Engine - Moon Disc Rendering (with phase)
//
// The Sun is rendered by Bevy's Atmosphere + SunDisk on the DirectionalLight.
// The Moon uses a CPU-generated phase texture on a StandardMaterial billboard.
// Phase is recalculated when the moon's elongation changes.
// ============================================================================

use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Marker for the moon disc entity
#[derive(Component)]
pub struct MoonDiscMarker;

/// Tracks the last rendered phase to avoid regenerating the texture every frame
#[derive(Component)]
pub struct MoonPhaseState {
    pub last_cos_phase: f32,
    pub last_waxing: bool,
}

pub struct SunDiscPlugin;

impl Plugin for SunDiscPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_moon_disc)
            .add_systems(Update, (sync_moon_disc_transform, update_moon_phase));
    }
}

/// Generate a moon phase RGBA texture on CPU.
/// Uses the terminator ellipse formula: x = cos(phase) * sqrt(1 - y²)
fn generate_moon_texture(size: u32, cos_phase: f32, waxing_sign: f32) -> Vec<u8> {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    let half = size as f32 / 2.0;

    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;

            // Map to [-1, 1] centered on disc
            let ux = (x as f32 - half) / half;
            let uy = (y as f32 - half) / half;
            let r = (ux * ux + uy * uy).sqrt();

            if r > 1.0 {
                // Outside disc — transparent
                continue;
            }

            // Soft edge antialiasing
            let edge_alpha = if r > 0.95 {
                1.0 - (r - 0.95) / 0.05
            } else {
                1.0
            };

            // Terminator: boundary between lit and dark halves
            let y_term = (1.0 - uy * uy).max(0.0).sqrt();
            let terminator = ux * waxing_sign - cos_phase * y_term;

            // Smooth transition at the terminator
            let lit = if terminator < -0.03 {
                1.0
            } else if terminator > 0.03 {
                0.0
            } else {
                0.5 - terminator / 0.06
            };

            // Lit side: bright. Dark side: faint earthshine
            let brightness = 0.05 + 0.95 * lit;

            // Limb darkening
            let limb = 1.0 - 0.15 * r * r;
            let b = brightness * limb;

            // Moon surface color (pale silver)
            pixels[idx] = (0.85 * b * 255.0) as u8;
            pixels[idx + 1] = (0.87 * b * 255.0) as u8;
            pixels[idx + 2] = (0.92 * b * 255.0) as u8;
            pixels[idx + 3] = (edge_alpha * 0.95 * 255.0) as u8;
        }
    }
    pixels
}

fn spawn_moon_disc(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let quad = meshes.add(Circle::new(0.5));

    // Generate initial quarter-moon texture (128x128 is plenty for a sky disc)
    let tex_size = 128;
    let pixels = generate_moon_texture(tex_size, 0.0, 1.0);

    let image = Image::new(
        Extent3d { width: tex_size, height: tex_size, depth_or_array_layers: 1 },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        default(),
    );
    let image_handle = images.add(image);

    let moon_mat = materials.add(StandardMaterial {
        base_color_texture: Some(image_handle.clone()),
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
        MoonPhaseState { last_cos_phase: 0.0, last_waxing: true },
        bevy::light::NotShadowCaster,
        bevy::light::NotShadowReceiver,
        Name::new("MoonDisc"),
    ));

    info!("🌙 Moon disc spawned with CPU phase texture");
}

/// Position the moon disc at sky distance from camera, always facing camera.
fn sync_moon_disc_transform(
    camera_query: Query<&GlobalTransform, (With<Camera3d>, Without<MoonDiscMarker>)>,
    sun_class: Query<&eustress_common::classes::Sun, With<eustress_common::services::lighting::Sun>>,
    moon_class: Query<&eustress_common::classes::Moon, With<eustress_common::services::lighting::Moon>>,
    mut moon_disc: Query<&mut Transform, With<MoonDiscMarker>>,
) {
    let Some(camera_gt) = camera_query.iter().next() else { return };
    let cam_pos = camera_gt.translation();

    let sun_data = sun_class.iter().next();
    let sun_dir = sun_data
        .map(|sc| sc.direction())
        .unwrap_or(Vec3::new(0.3, 0.8, 0.2).normalize());

    let sky_distance = 8000.0;

    let moon_data = moon_class.iter().next();
    let moon_dir = match (moon_data, sun_data) {
        (Some(moon), Some(sun)) => moon.direction_realistic(sun),
        _ => (-sun_dir).normalize(),
    };

    if let Ok(mut moon_t) = moon_disc.single_mut() {
        moon_t.translation = cam_pos + moon_dir * sky_distance;
        moon_t.look_at(cam_pos, Vec3::Y);
        let apparent_scale = 25.0 * (sky_distance / 500.0);
        moon_t.scale = if moon_dir.y < -0.02 { Vec3::ZERO } else { Vec3::splat(apparent_scale) };
    }
}

/// Regenerate moon phase texture when elongation changes significantly.
fn update_moon_phase(
    moon_class: Query<&eustress_common::classes::Moon, With<eustress_common::services::lighting::Moon>>,
    sun_class: Query<&eustress_common::classes::Sun, With<eustress_common::services::lighting::Sun>>,
    mut moon_disc: Query<(&MeshMaterial3d<StandardMaterial>, &mut MoonPhaseState), With<MoonDiscMarker>>,
    materials: Res<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(moon) = moon_class.iter().next() else { return };
    let Some(_sun) = sun_class.iter().next() else { return };
    let Ok((mat_handle, mut phase_state)) = moon_disc.single_mut() else { return };

    let elongation_deg = moon.elongation_from_sun();
    let elongation_rad = elongation_deg.to_radians();
    let cos_phase = elongation_rad.cos();
    let waxing = elongation_deg < 180.0;

    // Only regenerate when phase changes noticeably (saves CPU)
    if (cos_phase - phase_state.last_cos_phase).abs() < 0.02 && waxing == phase_state.last_waxing {
        return;
    }

    phase_state.last_cos_phase = cos_phase;
    phase_state.last_waxing = waxing;

    let waxing_sign = if waxing { 1.0 } else { -1.0 };

    // Get the texture handle from the material and update it in-place
    let Some(mat) = materials.get(&mat_handle.0) else { return };
    let Some(ref tex_handle) = mat.base_color_texture else { return };
    let Some(image) = images.get_mut(tex_handle) else { return };

    let tex_size = 128;
    let new_pixels = generate_moon_texture(tex_size, cos_phase, waxing_sign);

    if let Some(data) = image.data.as_mut() {
        if data.len() == new_pixels.len() {
            data.copy_from_slice(&new_pixels);
        }
    }
}
