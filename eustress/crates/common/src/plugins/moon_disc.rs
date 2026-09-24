//! # Moon disc
//!
//! The moon drawn as a lit sphere, not a texture of one.
//!
//! The shader reconstructs the sphere's surface under every pixel from the
//! view ray and lights it from the sun's actual direction, so the phase, the
//! tilt of the terminator and the way the crescent turns across the night
//! all come from the geometry [`crate::classes::Moon::direction_realistic`]
//! and [`crate::classes::Sun::direction`] already agree on. The surface has
//! the near side's maria in roughly their places, oriented to the moon's
//! celestial north so the face turns as the moon crosses the sky, plus
//! crater mottling, the bright ray craters, earthshine on the dark limb, and
//! a faint halo at night.
//!
//! ## Day and night
//!
//! By day the moon is behind a bright sky and is drawn additively, a pale
//! disc laid over the blue exactly as the real one is. At night it becomes
//! opaque and hides the stars behind its dark limb. Its brightness is set in
//! display terms, not in candela: the exposure adapts through the night (see
//! `sky_atmosphere`), and a physical moon would either vanish by day or
//! flatten to a featureless white disc at night.
//!
//! Replaces the engine's `SunDiscPlugin` moon: a CPU phase texture on an
//! alpha-blended quad, 2.9 degrees across, whose terminator was computed from
//! `lunar_day` alone and could face away from the sun.

use bevy::asset::embedded_asset;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

use crate::classes::{Moon as MoonClass, Sky, Sun as SunClass};
use crate::plugins::sky_atmosphere::{SkyBillboard, SkyCamera, SkyLight, SkyLightSet, SkyMedium};
use crate::services::lighting::{LightingService, Moon as MoonMarker, Sun as SunMarker};

/// Asset path of the embedded `moon_disc.wgsl`.
pub const MOON_DISC_SHADER_PATH: &str = "embedded://eustress_common/plugins/moon_disc.wgsl";

/// Distance the disc is drawn at, metres, capped at half the camera's far
/// plane. Only its direction matters; it just has to sit behind the scene.
const MOON_DISTANCE: f32 = 5_000.0;

/// The quad's half-width in disc radii. The halo reaches this far.
const HALO_EXTENT: f32 = 3.2;

/// Transparent draws sort by distance plus this bias, and draw from the most
/// negative up. The moon goes first so the cloud dome, sorted next (see
/// `volumetric_clouds`), passes in front of it.
pub const MOON_SORT_BIAS: f32 = -2.0e6;

/// Pre-tonemap brightness of the full, lit limb by day: a pale disc laid over
/// a sky that itself renders around 0.5.
const DAY_PEAK: f32 = 0.35;

/// Pre-tonemap brightness of the full, lit limb at night: bright enough to
/// read as the brightest thing in the sky and to bloom, dim enough that the
/// maria survive the tonemapper's shoulder.
const NIGHT_PEAK: f32 = 2.4;

/// Halo strength per unit of `Moon.glow_intensity`, at full night.
const HALO_GAIN: f32 = 0.35;

/// The disc's shader parameters: `MoonDiscParams` in `moon_disc.wgsl`,
/// binding 0. `moon_params_layout_matches_the_wgsl_struct` holds the two
/// layouts together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Reflect, ShaderType)]
pub struct MoonDiscParams {
    /// xyz: toward the moon's centre. w: sine of its angular radius.
    pub moon: Vec4,
    /// xyz: toward the sun. w: earthshine, as a fraction of the lit limb.
    pub sun: Vec4,
    /// xyz: the moon's celestial north, perpendicular to `moon`.
    /// w: how much the disc hides what is behind it, 0 by day, 1 at night.
    pub north: Vec4,
    /// rgb: pre-tonemap brightness of the full, lit limb, atmosphere
    /// included. a: halo strength.
    pub radiance: Vec4,
}

/// The moon's material. See the module docs.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct MoonDiscMaterial {
    #[uniform(0)]
    pub params: MoonDiscParams,
}

impl Material for MoonDiscMaterial {
    fn fragment_shader() -> ShaderRef {
        MOON_DISC_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        // Premultiplied: the shader's colour is added whole and `alpha` is
        // only how much of the sky behind the disc it hides.
        AlphaMode::Premultiplied
    }

    fn depth_bias(&self) -> f32 {
        MOON_SORT_BIAS
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // The disc is placed by direction, and a quad that happens to face
        // away must still draw.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// Marks the moon's disc.
#[derive(Component, Debug, Clone, Copy)]
pub struct MoonDisc;

/// Draws the moon. Added by `SharedLightingPlugin`.
pub struct MoonDiscPlugin;

impl Plugin for MoonDiscPlugin {
    fn build(&self, app: &mut App) {
        // A material plugin sets up its render side in `build`; without a
        // renderer there is nothing to draw.
        if app.get_sub_app(bevy::render::RenderApp).is_none() {
            return;
        }
        embedded_asset!(app, "moon_disc.wgsl");
        app.add_plugins(MaterialPlugin::<MoonDiscMaterial>::default())
            .add_systems(Update, sync_moon_disc.after(SkyLightSet));
    }
}

/// The moon's celestial north, as seen on its disc: the direction toward the
/// north celestial pole at `latitude`, made perpendicular to `toward_moon`.
fn disc_north(toward_moon: Vec3, latitude: f32) -> Vec3 {
    let lat = latitude.to_radians();
    let pole = Vec3::new(0.0, lat.sin(), lat.cos());
    (pole - toward_moon * pole.dot(toward_moon))
        .try_normalize()
        .unwrap_or_else(|| toward_moon.any_orthonormal_vector())
}

/// The disc's parameters for this frame.
pub fn moon_disc_params(
    moon: &MoonClass,
    sun: &SunClass,
    sky_light: &SkyLight,
    medium: &SkyMedium,
) -> MoonDiscParams {
    let toward_moon = sky_light.moon_direction;
    let radius = (moon.angular_size.clamp(0.05, 20.0) * 0.5).to_radians();
    // Earthshine is the Earth's own sunlit face lighting the moon's night
    // side, so it is brightest when the moon is new (the Earth, seen from
    // there, is full) and gone at full moon.
    let earthshine = moon.earthshine_intensity.max(0.0)
        * 0.5
        * (1.0 + moon.elongation_from_sun().to_radians().cos());
    let night = sky_light.night;
    let peak = DAY_PEAK + (NIGHT_PEAK - DAY_PEAK) * night;
    let tint = Color::srgb(moon.color[0], moon.color[1], moon.color[2]).to_linear();
    // Reddened and dimmed by the air between, like the sun: an orange
    // moonrise comes from the same medium that draws the sky.
    let air = medium.transmittance(0.0, toward_moon.y.max(0.0));
    let radiance = Vec3::new(tint.red, tint.green, tint.blue) * air * peak;
    MoonDiscParams {
        moon: toward_moon.extend(radius.sin()),
        sun: sky_light.sun_direction.extend(earthshine),
        north: disc_north(toward_moon, sun.latitude).extend(night),
        radiance: radiance.extend(moon.glow_intensity.max(0.0) * HALO_GAIN * night),
    }
}

/// Place the disc toward the moon from the main camera, and keep its
/// parameters current.
fn sync_moon_disc(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<MoonDiscMaterial>>,
    lighting: Res<LightingService>,
    sky_light: Res<SkyLight>,
    medium: Res<SkyMedium>,
    moons: Query<&MoonClass, With<MoonMarker>>,
    suns: Query<&SunClass, With<SunMarker>>,
    skies: Query<&Sky>,
    cameras: Query<(&Camera, &GlobalTransform, Option<&Projection>), With<SkyCamera>>,
    mut discs: Query<(&MeshMaterial3d<MoonDiscMaterial>, &mut Transform), With<MoonDisc>>,
    mut last: Local<Option<MoonDiscParams>>,
) {
    let Some((_, view, projection)) = cameras
        .iter()
        .filter(|(camera, ..)| camera.is_active)
        .min_by_key(|(camera, ..)| camera.order)
    else {
        return;
    };
    let fallback_sun;
    let sun = match suns.iter().next() {
        Some(sun) => sun,
        None => {
            fallback_sun = SunClass {
                time_of_day: lighting.time_of_day * 24.0,
                latitude: lighting.geographic_latitude,
                ..default()
            };
            &fallback_sun
        }
    };
    // With no Moon in the Space there is no moon to draw.
    let Some(moon) = moons.iter().next() else {
        for (_, mut transform) in discs.iter_mut() {
            if transform.scale != Vec3::ZERO {
                transform.scale = Vec3::ZERO;
            }
        }
        return;
    };
    let shown = skies.iter().next().map_or(true, |s| s.celestial_bodies_shown);
    let toward_moon = sky_light.moon_direction;
    let (distance, perspective) = match projection {
        Some(Projection::Perspective(p)) => (MOON_DISTANCE.min(p.far * 0.5), true),
        Some(Projection::Orthographic(_)) => (MOON_DISTANCE, false),
        _ => (MOON_DISTANCE, true),
    };
    // Below the horizon, or in an orthographic view (parallel rays have no
    // "far away" to put a moon in), the disc collapses to nothing rather
    // than being hidden: `Visibility` belongs to the engine's orthographic
    // toggle, and two writers would fight over it.
    let visible = shown && moon.enabled && perspective && toward_moon.y > -0.03;

    let radius = (moon.angular_size.clamp(0.05, 20.0) * 0.5).to_radians();
    let side = 2.0 * distance * radius.tan() * HALO_EXTENT;
    let position = view.translation() + toward_moon * distance;
    let up = if toward_moon.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let placed = Transform::from_translation(position)
        .looking_to(toward_moon, up)
        .with_scale(if visible { Vec3::new(side, side, 1.0) } else { Vec3::ZERO });

    let params = moon_disc_params(moon, sun, &sky_light, &medium);

    match discs.iter_mut().next() {
        Some((material, mut transform)) => {
            if transform.translation.distance_squared(placed.translation) > 1e-4
                || transform.scale != placed.scale
                || transform.rotation.angle_between(placed.rotation) > 1e-5
            {
                *transform = placed;
            }
            if visible && *last != Some(params) {
                if let Some(mut disc) = materials.get_mut(&material.0) {
                    disc.params = params;
                    *last = Some(params);
                }
            }
        }
        None => {
            let material = materials.add(MoonDiscMaterial { params });
            *last = Some(params);
            commands.spawn((
                Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
                MeshMaterial3d(material),
                placed,
                NotShadowCaster,
                NotShadowReceiver,
                SkyBillboard,
                MoonDisc,
                Name::new("Moon Disc"),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::surface_material::wgsl_layout::{layout, struct_fields};
    use std::mem::size_of;

    const SHADER: &str = include_str!("moon_disc.wgsl");

    #[test]
    fn moon_params_layout_matches_the_wgsl_struct() {
        // Four vec4s: 64 bytes, each field on its own 16-byte row. The Rust
        // side is packed by `ShaderType`, the WGSL side by the shader; they
        // must agree or every parameter after the first mismatch is garbage.
        let (members, size, _) = layout(&struct_fields(SHADER, "MoonDiscParams"));
        let names: Vec<&str> = members.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["moon", "sun", "north", "radiance"]);
        assert_eq!(size, 64);
        assert_eq!(size_of::<MoonDiscParams>(), 64);
        assert_eq!(MoonDiscParams::min_size().get(), 64);
    }

    #[test]
    fn the_disc_is_oriented_to_celestial_north() {
        // Due south at the meridian, 45 degrees north: celestial north on the
        // disc points up and toward the pole, and is perpendicular to the
        // line of sight.
        let toward_moon = Vec3::new(0.0, 0.5, -0.866).normalize();
        let north = disc_north(toward_moon, 45.0);
        assert!(north.dot(toward_moon).abs() < 1e-5);
        assert!(north.y > 0.5, "north on a southern moon points up: {north:?}");
        assert!((north.length() - 1.0).abs() < 1e-5);
        // At the pole itself there is no unique north, but there is a valid
        // perpendicular.
        let at_pole = disc_north(Vec3::new(0.0, 45f32.to_radians().sin(), 45f32.to_radians().cos()), 45.0);
        assert!((at_pole.length() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn the_moon_is_brighter_at_night_and_hides_the_stars_only_then() {
        let moon = MoonClass::default();
        let sun = SunClass::default();
        let medium = SkyMedium::default();
        let mut sky = SkyLight { moon_direction: Vec3::new(0.0, 0.7, -0.7).normalize(), ..default() };
        sky.night = 0.0;
        let day = moon_disc_params(&moon, &sun, &sky, &medium);
        sky.night = 1.0;
        let night = moon_disc_params(&moon, &sun, &sky, &medium);
        assert!(night.radiance.y > day.radiance.y * 4.0);
        assert_eq!(day.north.w, 0.0, "by day the moon adds to the sky, it hides nothing");
        assert_eq!(night.north.w, 1.0, "at night the dark limb hides the stars behind it");
        assert_eq!(day.radiance.w, 0.0, "no halo by day");
    }

    #[test]
    fn a_low_moon_is_redder_than_a_high_one() {
        let moon = MoonClass::default();
        let sun = SunClass::default();
        let medium = SkyMedium::default();
        let at = |y: f32| {
            let dir = Vec3::new(0.0, y, -(1.0 - y * y).sqrt());
            let sky = SkyLight { moon_direction: dir, night: 1.0, ..default() };
            moon_disc_params(&moon, &sun, &sky, &medium).radiance
        };
        let high = at(0.9);
        let low = at(0.03);
        assert!(low.x / low.z > high.x / high.z * 1.5, "high {high:?} low {low:?}");
        assert!(low.y < high.y, "the air dims a low moon");
    }
}
