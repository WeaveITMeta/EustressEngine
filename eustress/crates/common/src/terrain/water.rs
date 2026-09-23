//! # Terrain water
//!
//! Every water surface over the terrain draws with one shared material,
//! [`WaterSurfaceMaterial`]: a `StandardMaterial` extended with the terrain
//! height texture (`surface_material::TerrainHeightTexture`) and the water's
//! colours ([`WaterSurfaceExtension`]). Its fragment shader,
//! `water_surface.wgsl` beside this file, is embedded in this crate so the
//! Client draws it too. Per pixel it measures the depth, the water's own
//! height less the terrain height under it, and shades by it: a clear tint
//! and a band of foam along the shore, deepening to the deep colour and
//! opacity; pixels over ground that stands above the water are dropped.
//! Detail ripples drift on still water and run along the mesh's flow
//! (`ATTRIBUTE_UV_1`, world XZ metres per second, zero for still water) on a
//! river. Bevy's standard lighting and fog apply, alpha blended.
//!
//! ## Surfaces
//! - **The ocean**: one plane at [`WaterConfig::sea_level`] over the
//!   terrain's footprint while [`WaterConfig::enabled`] (the ribbon's Water
//!   button).
//! - **Lakes**: every `TerrainWaterBody` instance floods the ground below its
//!   level from its position, inside its footprint (see `water_bodies`).
//! - **Rivers**: every River-mode `TerrainSpline` with WaterSurface on lays a
//!   sloped ribbon along its channel (see `water_bodies`).
//!
//! Water has no colliders: nothing swims or floats yet, and a body walks
//! through the surface onto the ground below. A host without a renderer
//! builds no water at all.
//!
//! [`WaterConfig`]'s colour tints all three, so a world's water is one colour.
//! The height texture is bound [`SURFACE_SETTLE_FRAMES`] frames after it is
//! made (see `surface_material`); until then, and over terrain without a
//! height raster, every pixel counts as [`WATER_FALLBACK_DEPTH`] deep.
//!
//! [`SURFACE_SETTLE_FRAMES`]: super::surface_material::SURFACE_SETTLE_FRAMES

use bevy::asset::embedded_asset;
use bevy::light::NotShadowCaster;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
// Explicit, so the log macros do not depend on the prelude's `bevy_log`
// feature (see `avatar::boot`).
use tracing::{debug, info};

use super::surface_material::{
    sync_terrain_height_textures, TerrainHeightTexture, TerrainHeightTextureRequest, TerrainSurfacePlugin,
};
use super::water_bodies::{sync_river_water, sync_water_bodies, RiverWaterState, WaterBodyState};
use super::{apply_terrain_dirty_chunks, TerrainConfig, TerrainDirtyChunks, TerrainRoot};

/// Asset path of the embedded `water_surface.wgsl`.
pub const WATER_SURFACE_SHADER_PATH: &str = "embedded://eustress_common/terrain/water_surface.wgsl";

/// [`WaterSurfaceParams::flags`] bit: the terrain height texture is bound.
pub const WATER_FLAG_TERRAIN_HEIGHT: u32 = 1;

/// Depth, metres, every pixel counts as without a height texture: deep
/// enough to show the deep colour's character, too deep for shore foam.
pub const WATER_FALLBACK_DEPTH: f32 = 4.0;
/// Depth, metres, at which the deep colour and opacity are reached.
pub const WATER_DEEP_DEPTH: f32 = 6.0;
/// Depth, metres, by which the shore foam has faded out.
pub const WATER_FOAM_DEPTH: f32 = 0.5;
/// Linear RGB shallow water leans toward: a clear green-blue.
const SHALLOW_TINT: [f32; 3] = [0.22, 0.52, 0.5];
/// How far shallow water leans from the deep colour toward [`SHALLOW_TINT`].
const SHALLOW_TINT_MIX: f32 = 0.5;
/// Opacity at the shore as a fraction of the deep water's.
const SHALLOW_ALPHA_FRACTION: f32 = 0.3;
/// Linear RGB of the shore foam, and how strongly it covers the water.
const FOAM_COLOR: [f32; 4] = [0.85, 0.9, 0.9, 0.8];
/// Ripple cells per metre of the coarser ripple octave: about 3 m ripples.
const DETAIL_REPEATS: f32 = 0.35;
/// Slope the ripples give the normal: gentle, so reflections wobble rather
/// than scatter.
const NORMAL_STRENGTH: f32 = 0.12;
/// Metres per second still water's ripples drift.
const DRIFT_SPEED: f32 = 0.3;
/// Seconds a flowing ripple pattern runs before it is blended into a fresh
/// copy, so a river's ripples never stretch however long they run. It must
/// divide 3600 s, the period of Bevy's default `Time` wrap that the shader's
/// `globals.time` wraps at; otherwise the phases jump once an hour. 1.5 s
/// does.
const FLOW_PERIOD: f32 = 1.5;

// ============================================================================
// 1. Water Configuration
// ============================================================================

/// Runtime water configuration resource
#[derive(Resource, Clone, Debug)]
pub struct WaterConfig {
    /// Whether the ocean plane is shown. Lakes and rivers are instances with
    /// their own Enabled.
    pub enabled: bool,
    /// Sea level in world Y coordinates
    pub sea_level: f32,
    /// Water mode: Static (plane) or Dynamic (simulation)
    pub mode: WaterMode,
    /// sRGB tint of deep water `[r, g, b, a]`, for every water surface.
    /// Shallow water leans from it toward a clear green-blue.
    pub color: [f32; 4],
    /// Opacity of deep water, multiplied by `color`'s alpha; shallow water is
    /// a fraction of it.
    pub opacity: f32,
    /// Wave animation speed (0.0 = still water)
    pub wave_speed: f32,
    /// Wave amplitude in world units
    pub wave_amplitude: f32,
}

impl Default for WaterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sea_level: 0.0,
            mode: WaterMode::Static,
            color: [0.1, 0.3, 0.6, 1.0],
            opacity: 0.9,
            wave_speed: 0.0,
            wave_amplitude: 0.0,
        }
    }
}

/// Water rendering mode
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WaterMode {
    /// Static translucent plane at sea_level, a single draw call
    #[default]
    Static,
    /// Dynamic simulation via realism crate (future)
    Dynamic,
}

// ============================================================================
// 2. The water material
// ============================================================================

/// The water's colours, the height texture's placement and the ripples:
/// `WaterSurfaceParams` in `water_surface.wgsl`, binding 101.
/// `water_params_layout_matches_the_wgsl_struct` holds the two layouts
/// together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Reflect, ShaderType)]
pub struct WaterSurfaceParams {
    /// Linear RGB of water at the shore, and its opacity there.
    pub shallow_color: Vec4,
    /// Linear RGB of deep water, and its opacity.
    pub deep_color: Vec4,
    /// Linear RGB of the shore foam, and how strongly it covers the water.
    pub foam_color: Vec4,
    /// World XZ of the height texture's first texel: the chunk grid's
    /// footprint minimum, as the material map's.
    pub terrain_origin: Vec2,
    /// World XZ size the height texture spans.
    pub terrain_extent: Vec2,
    /// Height texture texels per axis.
    pub height_size: UVec2,
    /// [`WATER_FLAG_TERRAIN_HEIGHT`] while the height texture is bound.
    pub flags: u32,
    /// [`WATER_FALLBACK_DEPTH`].
    pub fallback_depth: f32,
    /// [`WATER_DEEP_DEPTH`].
    pub deep_depth: f32,
    /// [`WATER_FOAM_DEPTH`].
    pub foam_depth: f32,
    /// [`DETAIL_REPEATS`].
    pub detail_repeats: f32,
    /// [`NORMAL_STRENGTH`].
    pub normal_strength: f32,
    /// [`DRIFT_SPEED`].
    pub drift_speed: f32,
    /// [`FLOW_PERIOD`].
    pub flow_period: f32,
}

impl WaterSurfaceParams {
    /// The water `config` describes, over `terrain` (a root's config and its
    /// height texture's size) when the height texture is bound.
    pub fn new(config: &WaterConfig, terrain: Option<(&TerrainConfig, UVec2)>) -> Self {
        let [r, g, b, a] = config.color;
        let deep = Color::srgb(r, g, b).to_linear();
        let deep_rgb = Vec3::new(deep.red, deep.green, deep.blue);
        let deep_alpha = (a * config.opacity).clamp(0.0, 1.0);
        let shallow_rgb = deep_rgb.lerp(Vec3::from(SHALLOW_TINT), SHALLOW_TINT_MIX);
        let (terrain_origin, terrain_extent, height_size, flags) = match terrain {
            Some((terrain, size)) if size.min_element() > 0 => {
                let (min, max) = terrain.footprint_xz();
                (min, (max - min).max(Vec2::splat(1e-3)), size, WATER_FLAG_TERRAIN_HEIGHT)
            }
            _ => (Vec2::ZERO, Vec2::ONE, UVec2::ZERO, 0),
        };
        Self {
            shallow_color: shallow_rgb.extend(deep_alpha * SHALLOW_ALPHA_FRACTION),
            deep_color: deep_rgb.extend(deep_alpha),
            foam_color: Vec4::from(FOAM_COLOR),
            terrain_origin,
            terrain_extent,
            height_size,
            flags,
            fallback_depth: WATER_FALLBACK_DEPTH,
            deep_depth: WATER_DEEP_DEPTH,
            foam_depth: WATER_FOAM_DEPTH,
            detail_repeats: DETAIL_REPEATS,
            normal_strength: NORMAL_STRENGTH,
            drift_speed: DRIFT_SPEED,
            flow_period: FLOW_PERIOD,
        }
    }
}

/// What the water shader adds to the `StandardMaterial` bindings. The binding
/// numbers and the sample type are the ones `water_surface.wgsl` declares;
/// change both together.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct WaterSurfaceExtension {
    /// The terrain root's height texture, read texel by texel with
    /// `textureLoad`: R32Float cannot be filtered on every adapter, so the
    /// binding is non-filterable. `None` until it is ready, when a fallback
    /// image stands in and the shader does not read it.
    #[texture(100, sample_type = "float", filterable = false, visibility(fragment))]
    pub terrain_height: Option<Handle<Image>>,
    /// Colours, the height texture's placement and the ripples.
    #[uniform(101)]
    pub params: WaterSurfaceParams,
}

impl MaterialExtension for WaterSurfaceExtension {
    fn fragment_shader() -> ShaderRef {
        WATER_SURFACE_SHADER_PATH.into()
    }
}

/// The water material. See the module docs.
pub type WaterSurfaceMaterial = ExtendedMaterial<StandardMaterial, WaterSurfaceExtension>;

/// The `StandardMaterial` under the water: alpha blended, smooth, with the
/// dielectric reflectance of water (F0 about 0.02), and drawn from both
/// sides so the surface shows from under it.
fn water_base_material() -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.06,
        metallic: 0.0,
        reflectance: 0.35,
        double_sided: true,
        cull_mode: None,
        ..default()
    }
}

/// Marks every entity drawn with the water material: the ocean plane, the
/// water body chunks and the river ribbons. While one exists the terrain
/// height texture is kept.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct WaterSurface;

/// The water material every water surface shares, made on first use.
#[derive(Resource, Debug, Default)]
pub struct WaterSurfaceAssets {
    material: Option<Handle<WaterSurfaceMaterial>>,
}

impl WaterSurfaceAssets {
    /// The shared water material, made in `materials` the first time. Its
    /// parameters and height texture are kept current by
    /// [`sync_water_material`].
    pub fn material(&mut self, materials: &mut Assets<WaterSurfaceMaterial>) -> Handle<WaterSurfaceMaterial> {
        self.material
            .get_or_insert_with(|| {
                materials.add(WaterSurfaceMaterial {
                    base: water_base_material(),
                    extension: WaterSurfaceExtension {
                        terrain_height: None,
                        params: WaterSurfaceParams::new(&WaterConfig::default(), None),
                    },
                })
            })
            .clone()
    }
}

/// Keep the shared water material in step with [`WaterConfig`] and the
/// terrain's height texture, binding the texture once it is ready, and ask
/// for the texture while any [`WaterSurface`] exists. The material is only
/// written when something differs, so an idle frame does not re-prepare it.
pub fn sync_water_material(
    water: Option<Res<WaterConfig>>,
    mut assets: ResMut<WaterSurfaceAssets>,
    materials: Option<ResMut<Assets<WaterSurfaceMaterial>>>,
    roots: Query<(&TerrainConfig, Option<&TerrainHeightTexture>), With<TerrainRoot>>,
    surfaces: Query<(), With<WaterSurface>>,
    request: Option<ResMut<TerrainHeightTextureRequest>>,
) {
    let wanted = !surfaces.is_empty();
    if let Some(mut request) = request {
        if request.wanted != wanted {
            request.wanted = wanted;
        }
    }
    let Some(mut materials) = materials else {
        return;
    };
    // Read through `Deref` first so a Space without water leaves it alone.
    if assets.material.is_none() && !wanted {
        return;
    }
    let handle = assets.material(&mut materials);
    let default_config = WaterConfig::default();
    let config = water.as_deref().unwrap_or(&default_config);
    let bound = roots
        .iter()
        .next()
        .and_then(|(terrain, texture)| Some((terrain, texture.filter(|texture| texture.is_ready())?)));
    let params = WaterSurfaceParams::new(config, bound.map(|(terrain, texture)| (terrain, texture.size())));
    let image = bound.map(|(_, texture)| texture.image().clone());
    let stale = materials
        .get(&handle)
        .is_some_and(|material| material.extension.params != params || material.extension.terrain_height != image);
    if stale {
        if let Some(mut material) = materials.get_mut(&handle) {
            material.extension.params = params;
            material.extension.terrain_height = image;
        }
    }
}

// ============================================================================
// 3. The ocean
// ============================================================================

/// Marker component for the water plane entity
#[derive(Component, Default)]
pub struct WaterPlane;

/// Spawn the ocean: a flat quad at `sea_level` Y, `size_xz` across and
/// centred on `center_xz` (see [`super::TerrainConfig::footprint_xz`] for the
/// terrain's rectangle, which is not centred on the origin), drawn with the
/// shared water `material`. Still water, so its flow is zero.
pub fn spawn_water_plane(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<WaterSurfaceMaterial>,
    sea_level: f32,
    center_xz: Vec2,
    size_xz: Vec2,
) -> Entity {
    let mut mesh: Mesh = Plane3d::default().mesh().size(size_xz.x, size_xz.y).into();
    let vertices = mesh.count_vertices();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.0f32, 0.0]; vertices]);

    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
        Transform::from_translation(Vec3::new(center_xz.x, sea_level, center_xz.y)),
        Visibility::default(),
        WaterPlane,
        WaterSurface,
        // Translucent water casts no shadow onto the ground it shows.
        NotShadowCaster,
        Name::new("WaterPlane"),
    )).id()
}

/// System to spawn/update water plane when terrain exists and water is enabled
///
/// Respawns the plane when the terrain's config changes (a new terrain
/// replacing the old one included), so it keeps covering the footprint.
pub fn water_sync_system(
    mut commands: Commands,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<WaterSurfaceMaterial>>>,
    mut assets: ResMut<WaterSurfaceAssets>,
    water_config: Option<Res<WaterConfig>>,
    terrain_query: Query<Ref<super::TerrainConfig>, With<super::TerrainRoot>>,
    water_query: Query<Entity, With<WaterPlane>>,
) {
    let Some(config) = water_config else { return };

    if !config.enabled {
        // Remove water plane if disabled
        for entity in water_query.iter() {
            commands.entity(entity).despawn();
        }
        return;
    }

    let (Some(mut meshes), Some(mut materials)) = (meshes, materials) else { return };
    let Ok(terrain_config) = terrain_query.single() else { return };
    if !water_query.is_empty() {
        if !terrain_config.is_changed() {
            return; // Water already covers this terrain
        }
        for entity in water_query.iter() {
            commands.entity(entity).despawn();
        }
    }

    // The chunk grid runs from -chunks_x * chunk_size to
    // (chunks_x + 1) * chunk_size, so its centre is half a chunk off the
    // origin on each axis.
    let (min, max) = terrain_config.footprint_xz();
    let size = max - min;
    let center = (min + max) * 0.5;

    let material = assets.material(&mut materials);
    spawn_water_plane(&mut commands, &mut meshes, material, config.sea_level, center, size);
    info!(
        "Water plane spawned at Y={:.1}, size={:.0}x{:.0}, centred on ({:.0}, {:.0})",
        config.sea_level, size.x, size.y, center.x, center.y
    );
}

/// System to update water plane position when sea_level changes
pub fn water_update_system(
    water_config: Option<Res<WaterConfig>>,
    mut water_query: Query<&mut Transform, With<WaterPlane>>,
) {
    let Some(config) = water_config else { return };
    if !config.is_changed() { return; }

    for mut transform in water_query.iter_mut() {
        transform.translation.y = config.sea_level;
    }
}

// ============================================================================
// 4. Plugin
// ============================================================================

/// Every water surface and the material they share: the ocean plane, water
/// bodies and river ribbons, the shader and its `MaterialPlugin`. Added by
/// the shared `TerrainPlugin` (the Client) and by the engine's terrain
/// plugin, each guarding against adding it twice.
pub struct TerrainWaterPlugin;

impl Plugin for TerrainWaterPlugin {
    fn build(&self, app: &mut App) {
        // The ribbon's Water button writes this whatever the host draws.
        app.init_resource::<WaterConfig>().init_resource::<TerrainDirtyChunks>();
        // A material plugin sets up its render side in `build`, so the render
        // sub-app must exist by now (added after DefaultPlugins, as both
        // hosts do). Water has no colliders, so without a renderer there is
        // nothing to build.
        if app.get_sub_app(bevy::render::RenderApp).is_none() {
            debug!(target: "eustress::terrain::water", "no renderer: no water surfaces are built");
            return;
        }
        // The height texture the water measures its depth against.
        if !app.is_plugin_added::<TerrainSurfacePlugin>() {
            app.add_plugins(TerrainSurfacePlugin);
        }
        embedded_asset!(app, "water_surface.wgsl");
        app.add_plugins(MaterialPlugin::<WaterSurfaceMaterial>::default())
            .init_resource::<WaterSurfaceAssets>()
            .init_resource::<WaterBodyState>()
            .init_resource::<RiverWaterState>()
            .add_systems(Update, (water_sync_system, water_update_system).chain())
            // After the dirty-chunk pass, so a lake or river follows the
            // ground and the channel that pass just baked, in the same frame.
            .add_systems(Update, (sync_water_bodies, sync_river_water).after(apply_terrain_dirty_chunks))
            .add_systems(
                Update,
                sync_water_material
                    .after(sync_terrain_height_textures)
                    .after(water_sync_system)
                    .after(sync_water_bodies)
                    .after(sync_river_water),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::surface_material::wgsl_layout::{f32_at, layout, offset_in, struct_fields, u32_at};
    use bevy::render::render_resource::encase::UniformBuffer;

    const SHADER: &str = include_str!("water_surface.wgsl");

    #[test]
    fn water_params_layout_matches_the_wgsl_struct() {
        let (members, size, _) = layout(&struct_fields(SHADER, "WaterSurfaceParams"));
        assert_eq!(
            members.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(),
            [
                "shallow_color",
                "deep_color",
                "foam_color",
                "terrain_origin",
                "terrain_extent",
                "height_size",
                "flags",
                "fallback_depth",
                "deep_depth",
                "foam_depth",
                "detail_repeats",
                "normal_strength",
                "drift_speed",
                "flow_period",
            ],
            "the shader's fields, in the Rust order"
        );
        assert_eq!(WaterSurfaceParams::min_size().get() as usize, size, "encase and the shader agree on the size");

        let params = WaterSurfaceParams {
            shallow_color: Vec4::new(0.1, 0.2, 0.3, 0.4),
            deep_color: Vec4::new(0.5, 0.6, 0.7, 0.8),
            foam_color: Vec4::new(0.9, 1.0, 1.1, 1.2),
            terrain_origin: Vec2::new(-1.5, -2.5),
            terrain_extent: Vec2::new(3.5, 4.5),
            height_size: UVec2::new(5, 6),
            flags: 7,
            fallback_depth: 8.0,
            deep_depth: 9.0,
            foam_depth: 10.0,
            detail_repeats: 11.0,
            normal_strength: 12.0,
            drift_speed: 13.0,
            flow_period: 14.0,
        };
        let mut buffer = UniformBuffer::new(Vec::<u8>::new());
        buffer.write(&params).unwrap();
        let bytes = buffer.into_inner();
        assert!(bytes.len() >= size);
        let at = |name: &str| offset_in(&members, name);
        assert_eq!(f32_at(&bytes, at("shallow_color") + 12), 0.4);
        assert_eq!(f32_at(&bytes, at("deep_color") + 8), 0.7);
        assert_eq!(f32_at(&bytes, at("foam_color")), 0.9);
        assert_eq!(f32_at(&bytes, at("terrain_origin") + 4), -2.5);
        assert_eq!(f32_at(&bytes, at("terrain_extent")), 3.5);
        assert_eq!(u32_at(&bytes, at("height_size") + 4), 6);
        assert_eq!(u32_at(&bytes, at("flags")), 7);
        assert_eq!(f32_at(&bytes, at("fallback_depth")), 8.0);
        assert_eq!(f32_at(&bytes, at("deep_depth")), 9.0);
        assert_eq!(f32_at(&bytes, at("foam_depth")), 10.0);
        assert_eq!(f32_at(&bytes, at("detail_repeats")), 11.0);
        assert_eq!(f32_at(&bytes, at("normal_strength")), 12.0);
        assert_eq!(f32_at(&bytes, at("drift_speed")), 13.0);
        assert_eq!(f32_at(&bytes, at("flow_period")), 14.0);

        // The bindings the Rust side declares are the shader's.
        assert!(SHADER.contains("@binding(100) var water_terrain_height: texture_2d<f32>;"));
        assert!(SHADER.contains("@binding(101) var<uniform> water_params: WaterSurfaceParams;"));
        assert_eq!(WATER_FLAG_TERRAIN_HEIGHT, 1);
        assert!(SHADER.contains("const WATER_FLAG_TERRAIN_HEIGHT: u32 = 1u;"));
    }

    #[test]
    fn params_place_the_height_texture_on_the_footprint_only_when_bound() {
        let terrain = TerrainConfig { chunk_size: 48.0, chunks_x: 3, chunks_z: 2, ..TerrainConfig::default() };
        let config = WaterConfig::default();
        let unbound = WaterSurfaceParams::new(&config, None);
        assert_eq!(unbound.flags, 0, "no texture, no depth reads");
        assert_eq!(unbound.height_size, UVec2::ZERO);
        assert_eq!(WaterSurfaceParams::new(&config, Some((&terrain, UVec2::ZERO))).flags, 0);

        let bound = WaterSurfaceParams::new(&config, Some((&terrain, UVec2::new(224, 160))));
        assert_eq!(bound.flags, WATER_FLAG_TERRAIN_HEIGHT);
        let (min, max) = terrain.footprint_xz();
        assert_eq!(bound.terrain_origin, min);
        assert_eq!(bound.terrain_extent, max - min);
        assert_eq!(bound.height_size, UVec2::new(224, 160));

        // Deep water is the configured colour at its opacity; the shore is
        // clearer and more transparent.
        let deep = Color::srgb(config.color[0], config.color[1], config.color[2]).to_linear();
        assert!((bound.deep_color.x - deep.red).abs() < 1e-6);
        assert!((bound.deep_color.w - config.color[3] * config.opacity).abs() < 1e-6);
        assert!(bound.shallow_color.w < bound.deep_color.w);
        assert!(bound.foam_depth < bound.fallback_depth, "no foam without a height texture");
    }
}
