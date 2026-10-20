//! # Volumetric clouds
//!
//! A raymarched cloud layer, driven by the [`Clouds`] class.
//!
//! Every pixel of the sky marches its view ray through a shell over the
//! curved planet (so the layer converges on the horizon the way a real cloud
//! deck does), reads cloud density from two tiling noise volumes, and lights
//! each sample from the sun or the moon through a short march toward it.
//! Light is Beer-Lambert extinction, a two-lobe Henyey-Greenstein phase (the
//! silver lining when the sun is behind a cloud), two octaves of approximate
//! multiple scattering plus a diffusion term (what makes a thick cumulus
//! bright white rather than grey), a powder term for the dark crevices of a
//! sun-facing cloud, and sky light that is bluer on the tops than under the
//! bases. Sunlight reaching the layer is taken through the same atmosphere
//! that draws the sky, at the base and at the top separately, so after
//! sunset the tops still glow pink while the bases have gone grey.
//!
//! ## How it is drawn
//!
//! An inside-out sphere around the camera, alpha blended in the transparent
//! pass after the atmosphere has drawn the sky. It reads the depth prepass
//! to stop at scene geometry, so a mountain in front of the clouds hides
//! them and the ground seen from above the layer is covered by them. On a
//! camera without a depth prepass it falls back to the depth test, and the
//! clouds are always behind geometry.
//!
//! Known limits: clouds cast no shadows on the ground, do not appear in
//! reflections or the environment map, and throw no light shafts.
//!
//! ## Noise
//!
//! Both volumes tile, and both are histogram-equalised on the CPU so a
//! texel value IS its quantile. `coverage` then means what it says: 0.45
//! puts cloud over about 45% of the layer's middle, whatever the noise's own
//! statistics. They are built on worker threads at startup; the clouds
//! appear when they land.
//!
//! `EUSTRESS_CLOUDS=0` turns clouds off; `EUSTRESS_CLOUD_QUALITY` is `low`,
//! `medium` (default) or `high`.

use bevy::asset::{embedded_asset, RenderAssetUsages};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin, MeshPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, Extent3d, Face, RenderPipelineDescriptor, ShaderType,
    SpecializedMeshPipelineError, TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;
use std::sync::OnceLock;
use tracing::{info, warn};

use crate::classes::{CloudCoverage, CloudLayerType, Clouds};
use crate::plugins::sky_atmosphere::{
    disc_visibility, luminance, SkyBillboard, SkyCamera, SkyLight, SkyLightSet, SkyMedium,
};

/// Asset path of the embedded `volumetric_clouds.wgsl`.
pub const CLOUD_SHADER_PATH: &str = "embedded://eustress_common/plugins/volumetric_clouds.wgsl";

/// Edge of the base-shape noise volume, texels.
pub const SHAPE_NOISE_SIZE: u32 = 64;

/// Edge of the detail noise volume, texels.
pub const DETAIL_NOISE_SIZE: u32 = 32;

/// World size of one tile of the base shape, metres, at `spread` 1. Its
/// coarsest Worley cells are a quarter of this: clusters about 2 km across.
const SHAPE_TILE: f32 = 9_000.0;

/// World size of one tile of the detail noise, metres. It erodes the edges
/// at the scale of tens of metres.
const DETAIL_TILE: f32 = 850.0;

/// World size of one tile of the coverage variation, metres, so the layer
/// has clear patches and denser banks instead of one uniform field.
const WEATHER_TILE: f32 = 60_000.0;

/// How far the coverage wanders around the authored value.
const WEATHER_CONTRAST: f32 = 0.6;

/// Clouds fade into the sky over this distance, metres: aerial perspective,
/// and the reason the march can stop well short of the horizon.
const FADE_DISTANCE: f32 = 38_000.0;

/// Radius of the dome the clouds are drawn on, metres. Only its directions
/// matter; it has to sit inside the far plane.
const DOME_RADIUS: f32 = 1_000.0;

/// Transparent draws sort by distance plus this bias and draw from the most
/// negative up: after the moon (`moon_disc::MOON_SORT_BIAS`), so the clouds
/// pass in front of it, and before everything else, so glass and water in
/// front of the sky draw over the clouds.
pub const CLOUD_SORT_BIAS: f32 = -1.0e6;

/// Albedo of the ground under the layer, for the light it throws back up
/// onto the cloud bases.
const GROUND_ALBEDO: f32 = 0.25;

/// Strength of the isotropic diffusion term: how bright the deep inside of
/// a sunlit cloud gets. Single scattering alone renders a thick cumulus at
/// about a tenth of its real brightness, grey against a blue sky.
const DIFFUSION: f32 = 2.4;

/// The sky's colour on the cloud tops, normalised to unit luminance.
const SKY_BLUE: Vec3 = Vec3::new(0.777, 1.016, 1.495);

/// Parameters of the cloud shader: `CloudParams` in `volumetric_clouds.wgsl`,
/// binding 0. `cloud_params_layout_matches_the_wgsl_struct` holds the two
/// layouts together. Lights are in lux and radiances in cd/m^2; the shader
/// applies the camera's exposure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Reflect, ShaderType)]
pub struct CloudParams {
    /// x: base altitude, y: top altitude, z: planet radius (metres),
    /// w: march limit (metres).
    pub layer: Vec4,
    /// x: coverage, y: extinction per metre at full density, z: layer type
    /// (0 cumulus, 1 cirrus, 2 stratus, 3 cumulonimbus, 4 altocumulus),
    /// w: detail erosion.
    pub shape: Vec4,
    /// x: 1 / shape tile, y: 1 / detail tile, z: 1 / weather tile (per
    /// metre), w: weather contrast.
    pub scales: Vec4,
    /// xy: base-shape wind offset, zw: weather wind offset (metres).
    pub wind: Vec4,
    /// xyz: detail wind offset (metres). w: fade distance (metres).
    pub detail_wind: Vec4,
    /// xy: horizontal coverage bias direction, z: bias strength,
    /// w: bias mode (0 toward a direction, 1 the horizon, 2 overhead).
    pub bias: Vec4,
    /// xyz: toward the light the clouds are lit by (sun, or moon at night).
    /// w: 1 while there is a light at all.
    pub light_direction: Vec4,
    /// rgb: that light at the cloud base, lux.
    pub light_base: Vec4,
    /// rgb: that light at the cloud top, lux.
    pub light_top: Vec4,
    /// rgb: sky radiance on the cloud tops, cd/m^2.
    pub ambient_top: Vec4,
    /// rgb: light from below on the cloud bases, cd/m^2.
    pub ambient_bottom: Vec4,
    /// rgb: cloud tint. a: diffusion strength.
    pub albedo: Vec4,
    /// x: most primary steps, y: light steps, z: powder strength,
    /// w: forward scattering `g`.
    pub quality: Vec4,
}

/// The cloud material. See the module docs.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct CloudMaterial {
    #[uniform(0)]
    pub params: CloudParams,
    /// Base shape (R, equalised) and coverage variation (G, equalised).
    #[texture(1, dimension = "3d", visibility(fragment))]
    #[sampler(2, visibility(fragment))]
    pub shape: Option<Handle<Image>>,
    /// Edge erosion (R, equalised).
    #[texture(3, dimension = "3d", visibility(fragment))]
    #[sampler(4, visibility(fragment))]
    pub detail: Option<Handle<Image>>,
}

impl Material for CloudMaterial {
    fn fragment_shader() -> ShaderRef {
        CLOUD_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        // Premultiplied: the shader returns the light the clouds add and, in
        // alpha, how much of the sky behind them they hide.
        AlphaMode::Premultiplied
    }

    fn depth_bias(&self) -> f32 {
        CLOUD_SORT_BIAS
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
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // The camera is inside the dome: draw its inner faces, once.
        descriptor.primitive.cull_mode = Some(Face::Front);
        // With a depth prepass the shader stops each ray at the scene
        // itself, so the dome must not be depth tested against it: the
        // clouds between the camera and far terrain would be lost.
        if key.mesh_key.contains(MeshPipelineKey::DEPTH_PREPASS) {
            if let Some(depth) = descriptor.depth_stencil.as_mut() {
                depth.depth_compare = Some(CompareFunction::Always);
                depth.depth_write_enabled = Some(false);
            }
        }
        Ok(())
    }
}

/// Marks the cloud dome.
#[derive(Component, Debug, Clone, Copy)]
pub struct CloudDome;

/// The two noise volumes, and the build that makes them.
#[derive(Resource, Default)]
pub struct CloudNoise {
    pub shape: Option<Handle<Image>>,
    pub detail: Option<Handle<Image>>,
    /// The build in flight. `Receiver` is not `Sync`, hence the mutex.
    pending: Option<std::sync::Mutex<std::sync::mpsc::Receiver<(Image, Image)>>>,
}

/// Wind offsets, accumulated and wrapped to their tiles so they never grow
/// large enough to cost the noise lookups precision.
#[derive(Default)]
struct CloudWind {
    shape: Vec2,
    weather: Vec2,
    detail: Vec3,
}

/// Draws the cloud layer. Added by `SharedLightingPlugin`.
pub struct VolumetricCloudsPlugin;

impl Plugin for VolumetricCloudsPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Clouds>();
        // A material plugin sets up its render side in `build`; without a
        // renderer there is nothing to draw.
        if app.get_sub_app(bevy::render::RenderApp).is_none() {
            return;
        }
        embedded_asset!(app, "volumetric_clouds.wgsl");
        app.add_plugins(MaterialPlugin::<CloudMaterial>::default())
            .init_resource::<CloudNoise>()
            .add_systems(Startup, start_cloud_noise)
            .add_systems(
                Update,
                (poll_cloud_noise, sync_cloud_dome.after(poll_cloud_noise).after(SkyLightSet)),
            );
    }
}

fn clouds_enabled() -> bool {
    static V: OnceLock<bool> = OnceLock::new();
    *V.get_or_init(|| match std::env::var("EUSTRESS_CLOUDS") {
        Ok(v) => !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "off" | "no"),
        Err(_) => true,
    })
}

/// Most primary steps, and light steps, for `EUSTRESS_CLOUD_QUALITY`.
fn cloud_quality() -> (f32, f32) {
    static V: OnceLock<(f32, f32)> = OnceLock::new();
    *V.get_or_init(|| {
        match std::env::var("EUSTRESS_CLOUD_QUALITY")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "low" => (48.0, 4.0),
            "high" => (160.0, 8.0),
            "" | "medium" => (96.0, 6.0),
            other => {
                warn!("clouds: EUSTRESS_CLOUD_QUALITY={other:?} is not low/medium/high, using medium");
                (96.0, 6.0)
            }
        }
    })
}

/// Start building the noise volumes on worker threads.
fn start_cloud_noise(mut noise: ResMut<CloudNoise>) {
    if !clouds_enabled() {
        info!("☁️ Volumetric clouds off (EUSTRESS_CLOUDS)");
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("eustress-cloud-noise".into())
        .spawn(move || {
            let started = std::time::Instant::now();
            let shape = noise_image(build_shape_noise(SHAPE_NOISE_SIZE), SHAPE_NOISE_SIZE);
            let detail = noise_image(build_detail_noise(DETAIL_NOISE_SIZE), DETAIL_NOISE_SIZE);
            info!("☁️ Cloud noise built in {:.2?}", started.elapsed());
            let _ = tx.send((shape, detail));
        });
    match spawned {
        Ok(_) => noise.pending = Some(std::sync::Mutex::new(rx)),
        Err(e) => warn!("clouds: noise build thread failed to spawn: {e}, no clouds"),
    }
}

/// Land the noise volumes when their build finishes.
fn poll_cloud_noise(mut noise: ResMut<CloudNoise>, mut images: ResMut<Assets<Image>>) {
    let received = {
        let Some(rx) = noise.pending.as_ref() else { return };
        match rx.lock() {
            Ok(rx) => match rx.try_recv() {
                Ok(pair) => Some(pair),
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
            },
            Err(_) => None,
        }
    };
    noise.pending = None;
    match received {
        Some((shape, detail)) => {
            noise.shape = Some(images.add(shape));
            noise.detail = Some(images.add(detail));
        }
        None => warn!("clouds: noise build ended without a result, no clouds"),
    }
}

/// A tiling RGBA8 volume sampled with trilinear filtering.
fn noise_image(data: Vec<u8>, size: u32) -> Image {
    let mut image = Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: size },
        TextureDimension::D3,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

// ============================================================================
// Noise (CPU, tiling)
// ============================================================================

#[inline]
fn hash4(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ (z as u32).wrapping_mul(0xcb1a_b31f)
        ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x2974_5c85);
    h ^= h >> 16;
    h
}

#[inline]
fn unit(h: u32) -> f32 {
    (h >> 8) as f32 / 16_777_216.0
}

/// Gradient noise that tiles every `period` lattice cells. About -1..1.
fn perlin(p: Vec3, period: i32, seed: u32) -> f32 {
    const GRADIENTS: [Vec3; 12] = [
        Vec3::new(1.0, 1.0, 0.0),
        Vec3::new(-1.0, 1.0, 0.0),
        Vec3::new(1.0, -1.0, 0.0),
        Vec3::new(-1.0, -1.0, 0.0),
        Vec3::new(1.0, 0.0, 1.0),
        Vec3::new(-1.0, 0.0, 1.0),
        Vec3::new(1.0, 0.0, -1.0),
        Vec3::new(-1.0, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 1.0),
        Vec3::new(0.0, -1.0, 1.0),
        Vec3::new(0.0, 1.0, -1.0),
        Vec3::new(0.0, -1.0, -1.0),
    ];
    let cell = p.floor();
    let f = p - cell;
    let fade = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let (cx, cy, cz) = (cell.x as i32, cell.y as i32, cell.z as i32);
    let corner = |dx: i32, dy: i32, dz: i32| -> f32 {
        let h = hash4(
            (cx + dx).rem_euclid(period),
            (cy + dy).rem_euclid(period),
            (cz + dz).rem_euclid(period),
            seed,
        );
        GRADIENTS[(h % 12) as usize].dot(f - Vec3::new(dx as f32, dy as f32, dz as f32))
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(corner(0, 0, 0), corner(1, 0, 0), fade.x);
    let x10 = lerp(corner(0, 1, 0), corner(1, 1, 0), fade.x);
    let x01 = lerp(corner(0, 0, 1), corner(1, 0, 1), fade.x);
    let x11 = lerp(corner(0, 1, 1), corner(1, 1, 1), fade.x);
    lerp(lerp(x00, x10, fade.y), lerp(x01, x11, fade.y), fade.z)
}

/// Inverted Worley (1 at a feature point, falling with distance to the
/// nearest), tiling every `cells` cells.
fn worley(p: Vec3, cells: i32, seed: u32) -> f32 {
    let cell = p.floor();
    let f = p - cell;
    let (cx, cy, cz) = (cell.x as i32, cell.y as i32, cell.z as i32);
    let mut nearest = f32::MAX;
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let h = hash4(
                    (cx + dx).rem_euclid(cells),
                    (cy + dy).rem_euclid(cells),
                    (cz + dz).rem_euclid(cells),
                    seed,
                );
                let feature = Vec3::new(
                    unit(h),
                    unit(h.wrapping_mul(0x85eb_ca6b) ^ 0x27d4_eb2f),
                    unit(h.wrapping_mul(0xc2b2_ae35) ^ 0x1656_67b1),
                );
                let offset = Vec3::new(dx as f32, dy as f32, dz as f32) + feature - f;
                nearest = nearest.min(offset.length_squared());
            }
        }
    }
    1.0 - nearest.sqrt().min(1.0)
}

/// Three octaves of inverted Worley from `cells` up.
fn worley_fbm(p01: Vec3, cells: i32, seed: u32) -> f32 {
    worley(p01 * cells as f32, cells, seed) * 0.625
        + worley(p01 * (cells * 2) as f32, cells * 2, seed ^ 0x51ed_270b) * 0.25
        + worley(p01 * (cells * 4) as f32, cells * 4, seed ^ 0x2f69_3b91) * 0.125
}

/// Four octaves of tiling gradient noise from `period` cells up, 0..1.
fn perlin_fbm(p01: Vec3, period: i32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amplitude = 1.0;
    let mut norm = 0.0;
    let mut cells = period;
    for octave in 0..4u32 {
        sum += perlin(p01 * cells as f32, cells, seed ^ octave.wrapping_mul(0x68e3_1da4)) * amplitude;
        norm += amplitude;
        amplitude *= 0.5;
        cells *= 2;
    }
    (sum / norm * 0.5 + 0.5).clamp(0.0, 1.0)
}

/// Evaluate `f` at the centre of every texel of a `size`^3 volume, one
/// z-slice band per worker thread.
fn fill_volume(size: u32, f: impl Fn(Vec3) -> f32 + Sync) -> Vec<f32> {
    let size = size as usize;
    let slice = size * size;
    let mut out = vec![0.0f32; slice * size];
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 16);
    let band = size.div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        for (chunk_index, chunk) in out.chunks_mut(band * slice).enumerate() {
            let f = &f;
            scope.spawn(move || {
                for (i, value) in chunk.iter_mut().enumerate() {
                    let z = chunk_index * band + i / slice;
                    let y = (i % slice) / size;
                    let x = i % size;
                    let p = (Vec3::new(x as f32, y as f32, z as f32) + 0.5) / size as f32;
                    *value = f(p);
                }
            });
        }
    });
    out
}

/// Replace every value by its quantile, so the result is uniform on 0..1.
/// Monotonic, so shapes (every level set) are kept exactly.
fn equalise(values: &mut [f32]) {
    let mut order: Vec<u32> = (0..values.len() as u32).collect();
    order.sort_unstable_by(|&a, &b| values[a as usize].total_cmp(&values[b as usize]));
    let n = (values.len().max(2) - 1) as f32;
    let mut ranked = vec![0.0f32; values.len()];
    for (rank, &index) in order.iter().enumerate() {
        ranked[index as usize] = rank as f32 / n;
    }
    values.copy_from_slice(&ranked);
}

#[inline]
fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// The base shape volume. R: Perlin-Worley billows, the Perlin dilated by
/// Worley and then eroded by finer Worley (Schneider's recipe), equalised.
/// G: slow gradient noise for the coverage variation, equalised.
pub fn build_shape_noise(size: u32) -> Vec<u8> {
    let mut shape = fill_volume(size, |p| {
        let billows = worley_fbm(p, 4, 0x1234_5678);
        let perlin = perlin_fbm(p, 4, 0x0bad_f00d);
        // Perlin, dilated by the Worley billows...
        let perlin_worley = billows + perlin * (1.0 - billows);
        // ...then eroded by finer Worley, which carves the cauliflower.
        let fine = worley_fbm(p, 8, 0x0dec_afe5);
        ((perlin_worley - (fine - 1.0)) / (2.0 - fine)).clamp(0.0, 1.0)
    });
    equalise(&mut shape);
    let mut weather = fill_volume(size, |p| perlin_fbm(p, 2, 0x5eed_c10d));
    equalise(&mut weather);
    let mut data = Vec::with_capacity(shape.len() * 4);
    for (s, w) in shape.iter().zip(weather.iter()) {
        data.extend_from_slice(&[to_u8(*s), to_u8(*w), 255, 255]);
    }
    data
}

/// The detail volume. R: three octaves of Worley, equalised.
pub fn build_detail_noise(size: u32) -> Vec<u8> {
    let mut detail = fill_volume(size, |p| worley_fbm(p, 4, 0x7a5c_e11d));
    equalise(&mut detail);
    let mut data = Vec::with_capacity(detail.len() * 4);
    for d in &detail {
        data.extend_from_slice(&[to_u8(*d), 255, 255, 255]);
    }
    data
}

// ============================================================================
// Parameters
// ============================================================================

/// Per-type look: layer type index, extinction per metre at density 0.5,
/// and detail erosion at softness 0.5.
fn layer_look(layer: CloudLayerType) -> (f32, f32, f32) {
    match layer {
        CloudLayerType::Cumulus => (0.0, 0.045, 0.36),
        CloudLayerType::Cirrus => (1.0, 0.006, 0.55),
        CloudLayerType::Stratus => (2.0, 0.035, 0.22),
        CloudLayerType::Cumulonimbus => (3.0, 0.06, 0.30),
        CloudLayerType::Altocumulus => (4.0, 0.03, 0.45),
    }
}

/// Coverage bias as `(direction x, direction z, strength, mode)`.
fn coverage_bias(clouds: &Clouds) -> Vec4 {
    let strength = clouds.coverage_bias.clamp(0.0, 1.0);
    // World axes: north is +Z, east is +X.
    match clouds.coverage_mode {
        CloudCoverage::Full | CloudCoverage::Scattered => Vec4::ZERO,
        CloudCoverage::Northern => Vec4::new(0.0, 1.0, strength, 0.0),
        CloudCoverage::Southern => Vec4::new(0.0, -1.0, strength, 0.0),
        CloudCoverage::Eastern => Vec4::new(1.0, 0.0, strength, 0.0),
        CloudCoverage::Western => Vec4::new(-1.0, 0.0, strength, 0.0),
        CloudCoverage::Horizon => Vec4::new(0.0, 0.0, strength, 1.0),
        CloudCoverage::Zenith => Vec4::new(0.0, 0.0, strength, 2.0),
    }
}

/// `light` from above the atmosphere, as it arrives at `altitude` metres
/// from a disc of `angular_radius` (radians) in `direction`. Averaged over
/// the disc's upper and lower limbs so a setting light fades rather than
/// switching off at the layer's own horizon, which sits below 0 degrees.
fn light_at(medium: &SkyMedium, light: Vec3, direction: Vec3, angular_radius: f32, altitude: f32) -> Vec3 {
    let mu = direction.y.clamp(-1.0, 1.0);
    let spread = angular_radius.sin();
    light * (medium.transmittance(altitude, mu + spread) + medium.transmittance(altitude, mu - spread)) * 0.5
}

/// The cloud shader's parameters for this frame.
pub fn cloud_params(
    clouds: &Clouds,
    sky_light: &SkyLight,
    medium: &SkyMedium,
    planet_radius: f32,
    wind: (Vec2, Vec2, Vec3),
    quality: (f32, f32),
) -> CloudParams {
    let base = clouds.altitude.max(50.0);
    let top = base + clouds.thickness.max(50.0);
    let middle = 0.5 * (base + top);
    let (kind, extinction, erosion) = layer_look(clouds.layer_type);
    let density = clouds.density.clamp(0.0, 1.0);
    let softness = clouds.softness.clamp(0.0, 1.0);
    let spread = clouds.spread.max(0.05) / clouds.noise_scale.max(0.05);

    // The key light: the sun, or the moon once it outshines the sun at the
    // layer. One light march per sample, toward whichever lights the cloud.
    const SUN_RADIUS: f32 = 0.004_65;
    let moon_radius = 0.009;
    let sun_middle = light_at(medium, sky_light.sun_light, sky_light.sun_direction, SUN_RADIUS, middle);
    let moon_middle = light_at(medium, sky_light.moon_light, sky_light.moon_direction, moon_radius, middle);
    let (direction, radius, light) = if luminance(sun_middle) >= luminance(moon_middle) {
        (sky_light.sun_direction, SUN_RADIUS, sky_light.sun_light)
    } else {
        (sky_light.moon_direction, moon_radius, sky_light.moon_light)
    };
    let (light_base, light_top) = if clouds.time_of_day_tinting {
        (
            light_at(medium, light, direction, radius, base),
            light_at(medium, light, direction, radius, top),
        )
    } else {
        // Untinted: the light's own colour, only dimmed as it sets.
        let fade = disc_visibility(direction.y, radius);
        (light * fade, light * fade)
    };
    let lit = luminance(light_base) + luminance(light_top) > 1e-6;

    // Sky light on the tops: the dome's average radiance, mostly sky blue
    // with a share of the key light's colour (the sky around a low sun is
    // warm too).
    let sky_radiance = sky_light.sky_lux / std::f32::consts::PI;
    let key_hue = if luminance(light_top) > 1e-6 { light_top / luminance(light_top) } else { SKY_BLUE };
    let ambient_top = sky_radiance * (SKY_BLUE * 0.75 + key_hue * 0.25);
    // From below: sunlight and skylight thrown back up by the ground, and
    // the horizon sky.
    let ground = GROUND_ALBEDO
        * (luminance(sky_light.sun_ground) * sky_light.sun_direction.y.max(0.0)
            + luminance(sky_light.moon_ground) * sky_light.moon_direction.y.max(0.0)
            + sky_light.sky_lux)
        / std::f32::consts::PI;
    let shadow = Color::srgb(clouds.shadow_color[0], clouds.shadow_color[1], clouds.shadow_color[2])
        .to_linear();
    let shadow = Vec3::new(shadow.red, shadow.green, shadow.blue);
    let shadow_hue = if luminance(shadow) > 1e-4 { shadow / luminance(shadow) } else { Vec3::ONE };
    let ambient_bottom =
        (Vec3::splat(ground) * Vec3::new(1.0, 0.97, 0.92) + ambient_top * 0.35) * shadow_hue.lerp(Vec3::ONE, 0.5);

    let tint = Color::srgb(clouds.color[0], clouds.color[1], clouds.color[2]).to_linear();

    CloudParams {
        layer: Vec4::new(base, top, planet_radius, FADE_DISTANCE * 2.2),
        shape: Vec4::new(
            if clouds.enabled { clouds.coverage.clamp(0.0, 1.0) } else { 0.0 },
            extinction * (0.3 + 1.4 * density),
            kind,
            erosion * (0.6 + 0.8 * softness),
        ),
        scales: Vec4::new(
            1.0 / (SHAPE_TILE * spread),
            1.0 / DETAIL_TILE,
            1.0 / WEATHER_TILE,
            if clouds.coverage_mode == CloudCoverage::Scattered { WEATHER_CONTRAST * 1.6 } else { WEATHER_CONTRAST },
        ),
        wind: Vec4::new(wind.0.x, wind.0.y, wind.1.x, wind.1.y),
        detail_wind: wind.2.extend(FADE_DISTANCE),
        bias: coverage_bias(clouds),
        light_direction: direction.extend(if lit { 1.0 } else { 0.0 }),
        light_base: light_base.extend(0.0),
        light_top: light_top.extend(0.0),
        ambient_top: ambient_top.extend(0.0),
        ambient_bottom: ambient_bottom.extend(0.0),
        albedo: Vec3::new(tint.red, tint.green, tint.blue).extend(DIFFUSION),
        // Softer clouds scatter less sharply forward.
        quality: Vec4::new(quality.0, quality.1, 0.6, 0.85 - 0.15 * softness),
    }
}

/// Keep the dome around the main camera and its parameters current.
fn sync_cloud_dome(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CloudMaterial>>,
    noise: Res<CloudNoise>,
    sky_light: Res<SkyLight>,
    medium: Res<SkyMedium>,
    scene: Res<crate::plugins::sky_atmosphere::SceneAtmosphere>,
    authored: Query<&Clouds>,
    cameras: Query<(&Camera, &GlobalTransform, Option<&Projection>), With<SkyCamera>>,
    mut domes: Query<(&MeshMaterial3d<CloudMaterial>, &mut Transform), With<CloudDome>>,
    mut wind: Local<CloudWind>,
) {
    let (Some(shape), Some(detail)) = (noise.shape.clone(), noise.detail.clone()) else {
        return;
    };
    let Some((_, view, projection)) = cameras
        .iter()
        .filter(|(camera, ..)| camera.is_active)
        .min_by_key(|(camera, ..)| camera.order)
    else {
        return;
    };

    // The Space's own Clouds if it has one, the default fair-weather layer
    // if not.
    let fallback;
    let clouds = match authored.iter().next() {
        Some(clouds) => clouds,
        None => {
            fallback = Clouds::default();
            &fallback
        }
    };
    // Parallel rays have no sky to march through.
    let (radius, perspective) = match projection {
        Some(Projection::Perspective(p)) => (DOME_RADIUS.min(p.far * 0.5), true),
        Some(Projection::Orthographic(_)) => (DOME_RADIUS, false),
        _ => (DOME_RADIUS, true),
    };
    let visible = clouds.enabled && clouds.coverage > 0.0 && perspective;

    // Wind, wrapped to each tile. The detail runs a little faster than the
    // shapes and rises slowly, so the edges churn as the clouds drift.
    let spread = clouds.spread.max(0.05) / clouds.noise_scale.max(0.05);
    let heading = clouds.wind_direction_vec();
    let travel = Vec2::new(heading.x, heading.z)
        * clouds.wind_speed
        * clouds.animation_speed.max(0.0)
        * time.delta_secs();
    let shape_tile = SHAPE_TILE * spread;
    wind.shape = (wind.shape - travel).rem_euclid(Vec2::splat(shape_tile));
    wind.weather = (wind.weather - travel * 0.5).rem_euclid(Vec2::splat(WEATHER_TILE));
    let rise = clouds.animation_speed.max(0.0) * 0.8 * time.delta_secs();
    wind.detail = (wind.detail - Vec3::new(travel.x, rise, travel.y) * Vec3::new(1.5, 1.0, 1.5))
        .rem_euclid(Vec3::splat(DETAIL_TILE));

    let params = cloud_params(
        clouds,
        &sky_light,
        &medium,
        scene.atmosphere.planet_radius.max(1_000.0),
        (wind.shape, wind.weather, wind.detail),
        cloud_quality(),
    );

    let placed = Transform::from_translation(view.translation())
        .with_scale(Vec3::splat(if visible { radius } else { 0.0 }));

    match domes.iter_mut().next() {
        Some((material, mut transform)) => {
            if transform.translation.distance_squared(placed.translation) > 1e-6
                || transform.scale != placed.scale
            {
                *transform = placed;
            }
            if visible {
                if let Some(mut dome) = materials.get_mut(&material.0) {
                    if dome.params != params {
                        dome.params = params;
                    }
                }
            }
        }
        None => {
            let material = materials.add(CloudMaterial {
                params,
                shape: Some(shape),
                detail: Some(detail),
            });
            commands.spawn((
                Mesh3d(meshes.add(Sphere::new(1.0).mesh().ico(3).unwrap_or_else(|_| Sphere::new(1.0).mesh().uv(48, 24)))),
                MeshMaterial3d(material),
                placed,
                NotShadowCaster,
                NotShadowReceiver,
                NoFrustumCulling,
                SkyBillboard,
                CloudDome,
                Name::new("Cloud Dome"),
            ));
            info!("☁️ Volumetric clouds ready");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::surface_material::wgsl_layout::{layout, struct_fields};
    use std::mem::size_of;

    const SHADER: &str = include_str!("volumetric_clouds.wgsl");

    #[test]
    fn cloud_params_layout_matches_the_wgsl_struct() {
        let (members, size, _) = layout(&struct_fields(SHADER, "CloudParams"));
        let names: Vec<&str> = members.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "layer",
                "shape",
                "scales",
                "wind",
                "detail_wind",
                "bias",
                "light_direction",
                "light_base",
                "light_top",
                "ambient_top",
                "ambient_bottom",
                "albedo",
                "quality",
            ]
        );
        assert_eq!(size, 13 * 16);
        assert_eq!(size_of::<CloudParams>(), 13 * 16);
        assert_eq!(CloudParams::min_size().get(), 13 * 16);
    }

    #[test]
    fn noise_tiles_seamlessly() {
        // The volumes repeat across the sky, so the value just past one face
        // must be the value at the opposite face, or every tile boundary is a
        // visible seam.
        for &(x, y, z) in &[(0.0f32, 0.3f32, 0.7f32), (0.25, 0.0, 0.5), (0.6, 0.9, 0.0)] {
            let a = Vec3::new(x, y, z);
            for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                let b = a + axis;
                let w = |p: Vec3| worley(p * 4.0, 4, 7);
                let g = |p: Vec3| perlin(p * 4.0, 4, 7);
                assert!((w(a) - w(b)).abs() < 1e-4, "worley seams on {axis:?}");
                assert!((g(a) - g(b)).abs() < 1e-4, "perlin seams on {axis:?}");
            }
        }
    }

    #[test]
    fn equalised_noise_makes_coverage_mean_what_it_says() {
        // A texel value is its own quantile, so the fraction of texels above
        // `1 - c` is `c`, whatever the raw noise's distribution.
        let data = build_detail_noise(16);
        let values: Vec<f32> = data.chunks_exact(4).map(|t| t[0] as f32 / 255.0).collect();
        for coverage in [0.2f32, 0.45, 0.8] {
            let above = values.iter().filter(|v| **v > 1.0 - coverage).count() as f32 / values.len() as f32;
            assert!((above - coverage).abs() < 0.03, "coverage {coverage} covers {above}");
        }
    }

    #[test]
    fn cloud_tops_keep_the_sunset_after_the_bases_lose_it() {
        // Sun a degree below the horizon: the ground is in the Earth's
        // shadow, the bases at 1.5 km are losing the last of the light, and
        // the tops at 2.7 km still see the sun and are lit red.
        let medium = SkyMedium::default();
        let clouds = Clouds::default();
        let dir = Vec3::new(0.0, -1.0f32.to_radians().sin(), 1.0).normalize();
        let sky = SkyLight {
            sun_direction: dir,
            sun_light: Vec3::splat(130_000.0),
            ..default()
        };
        let p = cloud_params(&clouds, &sky, &medium, 6_371_000.0, Default::default(), (96.0, 6.0));
        let top = p.light_top.truncate();
        let base = p.light_base.truncate();
        assert!(luminance(top) > luminance(base) * 1.5, "top {top:?} base {base:?}");
        assert!(top.x > top.z * 3.0, "the sunset light on the tops is red: {top:?}");
    }

    #[test]
    fn the_moon_lights_the_clouds_at_night() {
        let medium = SkyMedium::default();
        let clouds = Clouds::default();
        let sky = SkyLight {
            sun_direction: Vec3::new(0.0, -0.6, 0.8),
            sun_light: Vec3::splat(130_000.0),
            moon_direction: Vec3::new(0.0, 0.7, -0.714),
            moon_light: Vec3::splat(15.0),
            ..default()
        };
        let p = cloud_params(&clouds, &sky, &medium, 6_371_000.0, Default::default(), (96.0, 6.0));
        assert!((p.light_direction.truncate() - sky.moon_direction).length() < 1e-5);
        assert_eq!(p.light_direction.w, 1.0);
        assert!(luminance(p.light_top.truncate()) > 1.0);
    }

    #[test]
    fn a_disabled_layer_has_no_coverage() {
        let clouds = Clouds { enabled: false, ..Clouds::default() };
        let p = cloud_params(
            &clouds,
            &SkyLight::default(),
            &SkyMedium::default(),
            6_371_000.0,
            Default::default(),
            (96.0, 6.0),
        );
        assert_eq!(p.shape.x, 0.0);
    }
}
