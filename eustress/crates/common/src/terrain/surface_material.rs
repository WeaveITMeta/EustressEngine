//! The textured terrain material, and the systems that build it and put it
//! on the chunks.
//!
//! [`TerrainSurfaceMaterial`] is a `StandardMaterial` extended with the
//! terrain texture arrays, the slot records and the material map
//! ([`TerrainSurfaceExtension`]). Its fragment shader, `terrain_surface.wgsl`
//! beside this file, is embedded in this crate so the Client draws it too.
//! Per pixel it reads the material map with the bilinear footprint
//! `TerrainData::sample_height` uses, keeps the four strongest slots,
//! sharpens their blend by height, samples each slot's layer planar on flat
//! ground and triplanar on slopes, and runs Bevy's standard lighting (forward
//! or deferred), so shadows, fog and atmosphere work as on any other
//! surface. Volumetric chunks add their brick material from `ATTRIBUTE_UV_1`
//! (see `marching::brick_material_uv`), and every chunk's vertex-colour alpha
//! darkens the result by the baked AO and slope shade (see `mesh`).
//!
//! ## Which material a chunk wears
//!
//! Chunks always spawn with the vertex-colour `StandardMaterial`
//! ([`TerrainChunkMaterial`]). [`swap_terrain_chunk_materials`] moves every
//! chunk of a root onto the root's surface material once it is drawable, and
//! back when it stops being so. A root has a [`TerrainSurface`] while its
//! height raster carries a full material layer no wider than
//! [`MAX_MATERIAL_MAP_SIDE`] and texture arrays exist or are being built; the
//! surface is drawable once the arrays are bound and it has existed for
//! [`SURFACE_SETTLE_FRAMES`] frames. Until then, for procedural terrain
//! (which has no material layer), and whenever the arrays are unavailable,
//! disabled or failed, the chunks keep the vertex colours, which paint every
//! slot in its swatch colour.
//!
//! ## Settling
//!
//! Bevy re-prepares a material the frame it changes, dropping the old bind
//! group first, and nothing orders that after the upload of the images the
//! material points at. A material handed images added that same frame can
//! miss a frame, and every chunk wearing it vanishes for that frame. So new
//! images reach a material only [`SURFACE_SETTLE_FRAMES`] frames after they
//! were added: [`TerrainSurfaceBindings`] trails
//! `TerrainTextureArrays::current` by that much, and chunks wait as long
//! before wearing a new surface (whose material map is new too). Contents
//! that change in place need no wait: the material map after a paint and the
//! slot records after a slot edit keep their size, and Bevy writes such an
//! update into the existing GPU texture or buffer, so bind groups stay valid.
//!
//! ## Keeping it current
//!
//! - The slot records (one [`TerrainSlotRecord`] per slot id, in a storage
//!   buffer every surface shares) are rewritten when the slot table or the
//!   bound arrays change, always against the bound arrays, so layer indices
//!   never point into arrays the materials do not have yet.
//! - The material map is re-uploaded whole, at most once a frame, when a
//!   writer of `TerrainData::material_cache` marked it dirty (paint, undo,
//!   load) or replaced the cache; the flag is cleared past change detection
//!   so autosave does not take the upload for an edit. A root with terrain
//!   layers uploads its bake's map, and its bake's flag counts (see
//!   `layers::surface_data`).
//! - The raster mapping ([`TerrainSurfaceParams`]) follows the root's config.
//!
//! ## Height texture
//!
//! Beside the material map, a root can carry a [`TerrainHeightTexture`]: its
//! SURFACE heights (the bake when there is one) as world metres, one R32Float
//! texel per raster cell with the material map's raster mapping. The water
//! material reads it to measure how deep the water is over each pixel. It
//! exists while [`TerrainHeightTextureRequest::wanted`] says some water
//! surface is drawn, whether or not the ground is textured, and is uploaded
//! again (whole, at most every [`HEIGHT_UPLOAD_INTERVAL_SECS`]) after the
//! height stamps (`TerrainDirtyChunks::height_seq`) move, the raster is
//! replaced or the config changes. Like the material map, a new texture is only bindable
//! [`SURFACE_SETTLE_FRAMES`] frames after it was added
//! ([`TerrainHeightTexture::is_ready`]).

use bevy::asset::{embedded_asset, RenderAssetUsages};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;
// Explicit, so the log macros do not depend on the prelude's `bevy_log`
// feature (see `avatar::boot`).
use tracing::{debug, warn};

use super::material::MATERIAL_SLOT_COUNT;
use super::material_slots::{TerrainMaterialSlots, TerrainMaterialSlotsPlugin};
use super::texture_arrays::{
    slot_surface_in, TerrainSlotSurface, TerrainTextureArraySet, TerrainTextureArrays, TerrainTextureStatus,
};
use super::{
    new_terrain_chunk_material, surface_data, terrain_standard_material, Chunk, TerrainBaked, TerrainChunkMaterial,
    TerrainConfig, TerrainData, TerrainDirtyChunks, TerrainRoot,
};

/// Asset path of the embedded `terrain_surface.wgsl`.
pub const TERRAIN_SURFACE_SHADER_PATH: &str = "embedded://eustress_common/terrain/terrain_surface.wgsl";

/// Frames new images wait before a material binds them, and a new surface
/// before chunks wear it (see the module docs). The render world runs a
/// frame behind, and two frames leaves one to spare.
pub const SURFACE_SETTLE_FRAMES: u32 = 2;

/// Widest height raster, in texels per side, whose material map is uploaded:
/// wgpu's default `max_texture_dimension_2d`, which both hosts run with.
/// A larger raster keeps the vertex-colour material.
pub const MAX_MATERIAL_MAP_SIDE: u32 = 8192;

/// Texel format of the height texture: one world height in metres per raster
/// cell. Shaders read it with `textureLoad` and blend the four cells
/// themselves, since not every adapter can filter a 32-bit float texture.
pub const TERRAIN_HEIGHT_FORMAT: TextureFormat = TextureFormat::R32Float;

/// Shortest time between two uploads of a height texture whose heights keep
/// changing (a brush stroke beside a lake), seconds. The texture is uploaded
/// whole, so this bounds what a long stroke costs; the last change always
/// reaches the GPU once the interval has passed.
pub const HEIGHT_UPLOAD_INTERVAL_SECS: f64 = 0.1;

/// [`TerrainSurfaceParams::flags`] bit: the texture arrays are bound.
pub const TERRAIN_SURFACE_FLAG_TEXTURED: u32 = 1;

/// How far, in blended weight plus height, a slot may trail the leading one
/// at a pixel and still show (see the shader). A quarter keeps boundaries
/// soft over a cell or so while stones of the brighter material poke through.
pub const HEIGHT_BLEND_DEPTH: f32 = 0.25;

/// Shortest tiling a slot record takes, in metres per repeat: the slot
/// table's own lower limit.
const MIN_TILING_METRES: f32 = 0.05;
/// Floor under a layer's mean luminance when centring its height proxy, so
/// a near-black texture does not turn into an enormous scale.
const MIN_MEAN_LUMINANCE: f32 = 0.02;
/// Rec. 709 luma weights, the ones the shader measures albedo height with.
const LUMINANCE: [f32; 3] = [0.2126, 0.7152, 0.0722];

// ============================================================================
// GPU layouts
// ============================================================================

/// Where the material map lies in the world, and the shader's switches:
/// `TerrainSurfaceParams` in `terrain_surface.wgsl`, binding 106.
/// `surface_params_layout_matches_the_wgsl_struct` holds the two layouts
/// together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Reflect, ShaderType)]
pub struct TerrainSurfaceParams {
    /// World XZ of the raster's first texel: the chunk grid's footprint
    /// minimum.
    pub world_origin: Vec2,
    /// World XZ size the raster spans, the footprint's.
    pub world_extent: Vec2,
    /// Raster texels per axis, `TerrainData::cache_width` and `cache_height`.
    pub cache_size: UVec2,
    /// [`TERRAIN_SURFACE_FLAG_TEXTURED`] once the texture arrays are bound.
    pub flags: u32,
    /// [`HEIGHT_BLEND_DEPTH`].
    pub blend_depth: f32,
}

impl TerrainSurfaceParams {
    /// The mapping for a `cache_size` raster laid over `config`'s chunk grid.
    pub fn new(config: &TerrainConfig, cache_size: UVec2, textured: bool) -> Self {
        let (min, max) = config.footprint_xz();
        Self {
            world_origin: min,
            world_extent: (max - min).max(Vec2::splat(1e-3)),
            cache_size,
            flags: if textured { TERRAIN_SURFACE_FLAG_TEXTURED } else { 0 },
            blend_depth: HEIGHT_BLEND_DEPTH,
        }
    }

    /// The fractional raster texel the shader reads at world `(x, z)`: the
    /// CPU mirror of its `add_material_map`, which the tests hold to
    /// `height_query::world_to_uv` times `size - 1`, the mapping
    /// `TerrainData::sample_height` uses.
    pub fn raster_texel(&self, world_x: f32, world_z: f32) -> Vec2 {
        let uv = ((Vec2::new(world_x, world_z) - self.world_origin) / self.world_extent).clamp(Vec2::ZERO, Vec2::ONE);
        uv * self.cache_size.saturating_sub(UVec2::ONE).as_vec2()
    }
}

/// One slot's entry in the storage buffer the shader indexes by slot id:
/// `TerrainSlotRecord` in `terrain_surface.wgsl`, binding 105.
///
/// Packed by hand rather than through `ShaderType`, so its bytes can be
/// written into the buffer in place. `vec3<f32>` aligns to 16 bytes in WGSL,
/// which makes the struct 48 bytes with 16-byte alignment; the explicit
/// padding keeps the Rust struct the same size, so the array stride matches
/// too. `slot_record_layout_matches_the_wgsl_struct` checks every offset
/// against the shader source.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TerrainSlotRecord {
    /// Linear RGB: the tint on the layer's albedo, or the flat colour when
    /// `layer` is negative.
    pub albedo: [f32; 3],
    /// Texture-array layer, -1 to draw the slot flat.
    pub layer: i32,
    /// Texture repeats per metre of ground, the inverse of the slot's tiling.
    pub repeats_per_metre: f32,
    /// Perceptual roughness of a flat slot.
    pub roughness: f32,
    /// Multiplier on the ORM roughness channel of a textured slot.
    pub roughness_scale: f32,
    pub metallic: f32,
    /// Scale that brings the layer's mean albedo luminance to 0.5, so the
    /// height blend weighs every layer's relief alike; 0 for a flat slot.
    pub height_scale: f32,
    pub _pad: [f32; 3],
}

impl TerrainSlotRecord {
    /// The record of a slot drawn as `surface`, whose layer (when it has one)
    /// averages `layer_mean_albedo` in linear light.
    pub fn new(surface: &TerrainSlotSurface, layer_mean_albedo: Option<[f32; 3]>) -> Self {
        let layer = surface.layer.and_then(|layer| i32::try_from(layer).ok());
        let height_scale = match layer {
            Some(_) => {
                let mean = layer_mean_albedo.unwrap_or([0.5; 3]);
                let luminance: f32 = mean.iter().zip(LUMINANCE).map(|(c, w)| c * w).sum();
                0.5 / luminance.max(MIN_MEAN_LUMINANCE)
            }
            None => 0.0,
        };
        let tiling = if surface.tiling.is_finite() { surface.tiling.max(MIN_TILING_METRES) } else { 4.0 };
        Self {
            albedo: surface.albedo,
            layer: layer.unwrap_or(-1),
            repeats_per_metre: 1.0 / tiling,
            roughness: surface.roughness,
            roughness_scale: surface.roughness_scale,
            metallic: surface.metallic,
            height_scale,
            _pad: [0.0; 3],
        }
    }
}

/// The records of all 256 slots of `slots`, drawn with `arrays` (the arrays
/// the materials have bound; `None` draws every slot flat).
pub fn pack_slot_records(slots: &TerrainMaterialSlots, arrays: Option<&TerrainTextureArraySet>) -> Vec<TerrainSlotRecord> {
    (0..MATERIAL_SLOT_COUNT)
        .map(|slot| {
            let slot = u8::try_from(slot).unwrap_or(u8::MAX);
            let surface = slot_surface_in(slots, arrays, slot);
            let mean = surface
                .layer
                .and_then(|layer| arrays?.layers.get(layer as usize))
                .map(|layer| layer.mean_albedo_linear);
            TerrainSlotRecord::new(&surface, mean)
        })
        .collect()
}

/// The material map's texels: `TerrainData::material_cache` as raw RGBA8,
/// one `[id_a, id_b, blend_b, 0]` cell per texel in raster order, the bytes
/// a matmap PNG holds.
pub fn material_map_bytes(data: &TerrainData) -> Vec<u8> {
    data.material_cache.as_flattened().to_vec()
}

/// The material map's size when `data` can be drawn by the surface material:
/// a height raster with a full material layer, no wider than
/// [`MAX_MATERIAL_MAP_SIDE`] on either side. `None` keeps the vertex colours.
pub fn surface_raster_size(data: &TerrainData) -> Option<UVec2> {
    if data.height_cache.is_empty() || !data.has_material_layer() {
        return None;
    }
    let size = UVec2::new(data.cache_width, data.cache_height);
    (size.max_element() <= MAX_MATERIAL_MAP_SIDE).then_some(size)
}

/// The material map texture of `data`, `size` texels. Unorm, not sRGB, so
/// the shader reads each byte back exactly as `value * 255`; render-world
/// only, since `material_cache` is the CPU copy.
fn material_map_image(data: &TerrainData, size: UVec2) -> Image {
    Image::new(
        Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 },
        TextureDimension::D2,
        material_map_bytes(data),
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// The height texture's size for `data`: its whole height raster, no wider
/// than [`MAX_MATERIAL_MAP_SIDE`] on either side. `None` without one
/// (procedural terrain) or when it is too wide for a texture.
pub fn height_texture_size(data: &TerrainData) -> Option<UVec2> {
    let size = UVec2::new(data.cache_width, data.cache_height);
    if size.min_element() == 0 || data.height_cache.len() != size.x as usize * size.y as usize {
        return None;
    }
    (size.max_element() <= MAX_MATERIAL_MAP_SIDE).then_some(size)
}

/// The height texture's texels: every cell of `data`'s height raster as its
/// world height in metres ([`TerrainConfig::world_height`]), native-endian
/// f32 in raster order, the order the material map's cells are in. World
/// metres rather than the normalized samples, so a Save that moves the
/// height band (re-expressing the samples, not moving the ground) leaves
/// the texture right. A sample that is not finite reads as the band's floor.
pub fn height_map_bytes(config: &TerrainConfig, data: &TerrainData) -> Vec<u8> {
    let floor = config.world_height(0.0);
    // Written straight into the bytes, in one pass and one allocation: this
    // runs on every upload of a raster that can be tens of MB.
    let mut bytes = Vec::with_capacity(data.height_cache.len() * std::mem::size_of::<f32>());
    for &sample in &data.height_cache {
        let world = config.world_height(sample);
        bytes.extend_from_slice(&(if world.is_finite() { world } else { floor }).to_ne_bytes());
    }
    bytes
}

/// The height texture of `data`, `size` texels; render-world only, since
/// `height_cache` is the CPU copy.
fn height_map_image(config: &TerrainConfig, data: &TerrainData, size: UVec2) -> Image {
    Image::new(
        Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 },
        TextureDimension::D2,
        height_map_bytes(config, data),
        TERRAIN_HEIGHT_FORMAT,
        RenderAssetUsages::RENDER_WORLD,
    )
}

// ============================================================================
// The material
// ============================================================================

/// What the terrain shader adds to the `StandardMaterial` bindings. The
/// binding numbers, sample types and array views are the ones
/// `terrain_surface.wgsl` declares; change both together.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct TerrainSurfaceExtension {
    /// sRGB albedo array. `None` until the arrays are bound, when a fallback
    /// image stands in and the shader does not read it. Its sampler (repeat,
    /// trilinear, anisotropic) serves all three arrays.
    #[texture(100, dimension = "2d_array", visibility(fragment))]
    #[sampler(103, visibility(fragment))]
    pub albedo: Option<Handle<Image>>,
    /// Tangent-space normal array.
    #[texture(101, dimension = "2d_array", visibility(fragment))]
    pub normal: Option<Handle<Image>>,
    /// Occlusion, roughness, metallic array.
    #[texture(102, dimension = "2d_array", visibility(fragment))]
    pub orm: Option<Handle<Image>>,
    /// The root's material map, read texel by texel with `textureLoad`.
    #[texture(104, visibility(fragment))]
    pub material_map: Option<Handle<Image>>,
    /// [`MATERIAL_SLOT_COUNT`] [`TerrainSlotRecord`]s, shared by every
    /// surface.
    #[storage(105, read_only, visibility(fragment))]
    pub slot_records: Handle<ShaderBuffer>,
    /// Where the material map lies, and whether the arrays are bound.
    #[uniform(106)]
    pub params: TerrainSurfaceParams,
}

impl MaterialExtension for TerrainSurfaceExtension {
    fn fragment_shader() -> ShaderRef {
        TERRAIN_SURFACE_SHADER_PATH.into()
    }

    // The same file, compiled with PREPASS_PIPELINE, writes the gbuffer when
    // the app renders deferred (`EUSTRESS_SSR`).
    fn deferred_fragment_shader() -> ShaderRef {
        TERRAIN_SURFACE_SHADER_PATH.into()
    }
}

/// The textured terrain material. See the module docs.
pub type TerrainSurfaceMaterial = ExtendedMaterial<StandardMaterial, TerrainSurfaceExtension>;

/// Whether `extension` already binds `bound`'s arrays with `params`.
fn extension_binds(extension: &TerrainSurfaceExtension, params: &TerrainSurfaceParams, bound: Option<&TerrainTextureArraySet>) -> bool {
    extension.params == *params
        && extension.albedo.as_ref() == bound.map(|set| &set.albedo)
        && extension.normal.as_ref() == bound.map(|set| &set.normal)
        && extension.orm.as_ref() == bound.map(|set| &set.orm)
}

/// Point `extension` at `bound`'s arrays (none when `None`) with `params`.
fn bind_extension(extension: &mut TerrainSurfaceExtension, params: TerrainSurfaceParams, bound: Option<&TerrainTextureArraySet>) {
    extension.params = params;
    extension.albedo = bound.map(|set| set.albedo.clone());
    extension.normal = bound.map(|set| set.normal.clone());
    extension.orm = bound.map(|set| set.orm.clone());
}

// ============================================================================
// State
// ============================================================================

/// What every surface material binds in common: the texture arrays, trailing
/// `TerrainTextureArrays::current` by [`SURFACE_SETTLE_FRAMES`], and the slot
/// records buffer computed against them. See the module docs.
#[derive(Resource, Default)]
pub struct TerrainSurfaceBindings {
    arrays: Option<TerrainTextureArraySet>,
    /// The `TerrainTextureArrays::generation` `arrays` was taken at.
    generation: Option<u64>,
    /// A newer generation, and the frames it has waited.
    pending: Option<(u64, u32)>,
    slot_records: Option<Handle<ShaderBuffer>>,
    records_stale: bool,
}

impl TerrainSurfaceBindings {
    /// The arrays the surface materials bind, `None` when there are none.
    pub fn arrays(&self) -> Option<&TerrainTextureArraySet> {
        self.arrays.as_ref()
    }

    /// The slot records buffer, once created.
    pub fn slot_records(&self) -> Option<&Handle<ShaderBuffer>> {
        self.slot_records.as_ref()
    }
}

/// A terrain root's surface material and material map. Present while the
/// root can be drawn textured or soon will be (see the module docs); removed
/// when it cannot, which drops both.
#[derive(Component, Debug)]
pub struct TerrainSurface {
    material: Handle<TerrainSurfaceMaterial>,
    material_map: Handle<Image>,
    map_size: UVec2,
    /// Address of the `material_cache` allocation last uploaded, so a cache
    /// replaced wholesale (a baseline restored with the dirty flag clear)
    /// still reaches the GPU.
    uploaded_cache: usize,
    /// The material has the texture arrays bound.
    textured: bool,
    /// Frames since this surface was created.
    age_frames: u32,
}

impl TerrainSurface {
    /// The material the root's chunks wear while [`Self::is_drawable`].
    pub fn material(&self) -> &Handle<TerrainSurfaceMaterial> {
        &self.material
    }

    /// Chunks may wear it: the arrays are bound, and the surface is old
    /// enough for its material map to be on the GPU.
    pub fn is_drawable(&self) -> bool {
        self.textured && self.age_frames >= SURFACE_SETTLE_FRAMES
    }
}

/// Whether anything draws with a terrain height texture: set by the water
/// systems while a water surface exists, so a Space without water pays
/// neither the texture's memory nor its uploads.
#[derive(Resource, Debug, Default)]
pub struct TerrainHeightTextureRequest {
    pub wanted: bool,
}

/// A terrain root's surface heights as a texture (see the module docs).
/// Present while [`TerrainHeightTextureRequest::wanted`] and the root has a
/// height raster no wider than [`MAX_MATERIAL_MAP_SIDE`].
#[derive(Component, Debug)]
pub struct TerrainHeightTexture {
    image: Handle<Image>,
    size: UVec2,
    /// Address of the `height_cache` allocation last uploaded: a raster
    /// replaced wholesale, or the switch between the base and a bake, marks
    /// nothing in `TerrainDirtyChunks`.
    uploaded_cache: usize,
    /// `TerrainDirtyChunks::height_seq` at the last upload.
    uploaded_seq: u64,
    /// `Time<Real>` seconds of the last upload, `None` without a clock.
    uploaded_at: Option<f64>,
    /// The heights changed since the last upload.
    stale: bool,
    /// Frames since this texture was created.
    age_frames: u32,
}

impl TerrainHeightTexture {
    /// The R32Float image, [`Self::size`] texels.
    pub fn image(&self) -> &Handle<Image> {
        &self.image
    }

    /// Texels per axis: the raster's `cache_width` and `cache_height`.
    pub fn size(&self) -> UVec2 {
        self.size
    }

    /// A material may bind it: it is old enough to be on the GPU.
    pub fn is_ready(&self) -> bool {
        self.age_frames >= SURFACE_SETTLE_FRAMES
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Advance [`TerrainSurfaceBindings`]: take up newly published arrays once
/// they have settled (and drop released ones at once), and rewrite the slot
/// records when the slot table or the bound arrays changed. The records keep
/// one length, so after the first write they are updated in place.
pub fn settle_terrain_surface_bindings(
    arrays: Res<TerrainTextureArrays>,
    slots: Res<TerrainMaterialSlots>,
    mut bindings: ResMut<TerrainSurfaceBindings>,
    buffers: Option<ResMut<Assets<ShaderBuffer>>>,
) {
    let Some(mut buffers) = buffers else {
        return;
    };
    let bindings = &mut *bindings;

    let live = arrays.generation();
    if bindings.generation != Some(live) {
        match arrays.current() {
            None => {
                // Released or never built: nothing to wait for.
                bindings.arrays = None;
                bindings.generation = Some(live);
                bindings.pending = None;
                bindings.records_stale = true;
            }
            Some(current) => {
                let waited = match bindings.pending {
                    Some((generation, frames)) if generation == live => frames + 1,
                    _ => 0,
                };
                if waited >= SURFACE_SETTLE_FRAMES {
                    bindings.arrays = Some(current.clone());
                    bindings.generation = Some(live);
                    bindings.pending = None;
                    bindings.records_stale = true;
                } else {
                    bindings.pending = Some((live, waited));
                }
            }
        }
    }

    if slots.is_changed() || bindings.slot_records.is_none() {
        bindings.records_stale = true;
    }
    if !bindings.records_stale {
        return;
    }
    bindings.records_stale = false;
    let records = pack_slot_records(&slots, bindings.arrays.as_ref());
    let bytes: Vec<u8> = bytemuck::cast_slice(&records).to_vec();
    if let Some(handle) = bindings.slot_records.clone() {
        if let Some(mut buffer) = buffers.get_mut(&handle) {
            buffer.data = Some(bytes);
            return;
        }
    }
    bindings.slot_records = Some(buffers.add(ShaderBuffer::new(&bytes, RenderAssetUsages::RENDER_WORLD)));
}

/// Give every eligible terrain root a [`TerrainSurface`] and keep it current:
/// the material map re-uploaded (whole, once a frame at most) after its
/// cells change, the arrays and raster mapping rebound when they change. A
/// root that stops being eligible loses its surface. Clears
/// `TerrainData::material_dirty` past change detection once the upload has
/// taken it. A root with terrain layers draws its bake: the map, its size
/// and its dirty flag all come from [`surface_data`], and the base's flag is
/// cleared with the bake's, since every base change reaches the bake first.
pub fn sync_terrain_surfaces(
    mut commands: Commands,
    bindings: Res<TerrainSurfaceBindings>,
    arrays: Res<TerrainTextureArrays>,
    mut materials: ResMut<Assets<TerrainSurfaceMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut roots: Query<
        (Entity, &TerrainConfig, &mut TerrainData, Option<&mut TerrainBaked>, Option<&mut TerrainSurface>),
        With<TerrainRoot>,
    >,
    mut warned_oversized: Local<bool>,
) {
    // Worth a surface while arrays exist or the first build runs, so it is
    // prepared by the time they settle; not when they never will.
    let wanted = bindings.arrays.is_some()
        || arrays.current().is_some()
        || matches!(arrays.status(), TerrainTextureStatus::Building);
    let bound = bindings.arrays.as_ref();

    for (entity, config, mut base_data, mut baked, surface) in &mut roots {
        let data = surface_data(&base_data, baked.as_deref());
        let size = surface_raster_size(data);
        if size.is_none() && data.has_material_layer() && !data.height_cache.is_empty() && !*warned_oversized {
            *warned_oversized = true;
            warn!(
                target: "eustress::terrain::textures",
                width = data.cache_width,
                height = data.cache_height,
                max = MAX_MATERIAL_MAP_SIDE,
                "terrain raster is too wide for a material-map texture; it keeps the vertex-colour material"
            );
        }
        let (true, Some(size), Some(slot_records)) = (wanted, size, bindings.slot_records.clone()) else {
            if surface.is_some() {
                commands.entity(entity).try_remove::<TerrainSurface>();
            }
            continue;
        };
        let params = TerrainSurfaceParams::new(config, size, bound.is_some());
        let cache_id = data.material_cache.as_ptr() as usize;

        match surface {
            Some(mut surface) if surface.map_size == size => {
                surface.age_frames = surface.age_frames.saturating_add(1);
                surface.textured = bound.is_some();
                if data.material_dirty || surface.uploaded_cache != cache_id {
                    // Same size, so Bevy writes into the existing texture
                    // and the material's bind group stays valid.
                    if let Some(mut image) = images.get_mut(&surface.material_map) {
                        image.data = Some(material_map_bytes(&data));
                    }
                    surface.uploaded_cache = cache_id;
                }
                let rebind = materials
                    .get(&surface.material)
                    .is_some_and(|material| !extension_binds(&material.extension, &params, bound));
                if rebind {
                    if let Some(mut material) = materials.get_mut(&surface.material) {
                        bind_extension(&mut material.extension, params, bound);
                    }
                }
            }
            _ => {
                // First surface, or the raster changed size: a new map
                // texture, so a new surface that waits out its settle
                // frames (chunks show vertex colours meanwhile).
                let material_map = images.add(material_map_image(&data, size));
                let mut extension = TerrainSurfaceExtension {
                    albedo: None,
                    normal: None,
                    orm: None,
                    material_map: Some(material_map.clone()),
                    slot_records,
                    params,
                };
                bind_extension(&mut extension, params, bound);
                let material = materials.add(TerrainSurfaceMaterial { base: terrain_standard_material(), extension });
                commands.entity(entity).try_insert(TerrainSurface {
                    material,
                    material_map,
                    map_size: size,
                    uploaded_cache: cache_id,
                    textured: bound.is_some(),
                    age_frames: 0,
                });
            }
        }
        if base_data.material_dirty {
            base_data.bypass_change_detection().material_dirty = false;
        }
        if let Some(baked) = baked.as_mut() {
            if baked.data.material_dirty {
                baked.bypass_change_detection().data.material_dirty = false;
            }
        }
    }
}

/// Put every chunk of a root on the root's surface material while it is
/// drawable, and back on the vertex-colour material (the root's
/// [`TerrainChunkMaterial`], made if missing) otherwise. Runs after the
/// systems that spawn chunks, which always spawn on the vertex-colour
/// material, so a chunk spawned into a textured terrain is moved before it
/// is ever drawn.
pub fn swap_terrain_chunk_materials(
    mut commands: Commands,
    mut standard_materials: ResMut<Assets<StandardMaterial>>,
    roots: Query<
        (Entity, &TerrainConfig, Option<&TerrainSurface>, Option<&TerrainChunkMaterial>, Option<&Children>),
        With<TerrainRoot>,
    >,
    chunks: Query<(Has<MeshMaterial3d<StandardMaterial>>, Option<&MeshMaterial3d<TerrainSurfaceMaterial>>), With<Chunk>>,
) {
    for (root, config, surface, chunk_material, children) in &roots {
        let Some(children) = children else {
            continue;
        };
        let drawable = surface.filter(|surface| surface.is_drawable()).map(TerrainSurface::material);
        let mut fallback = chunk_material.map(|material| material.0.clone());
        for &child in children {
            let Ok((has_standard, textured)) = chunks.get(child) else {
                continue;
            };
            match drawable {
                Some(material) => {
                    if has_standard || textured.is_none_or(|current| current.0 != *material) {
                        commands
                            .entity(child)
                            .try_remove::<MeshMaterial3d<StandardMaterial>>()
                            .try_insert(MeshMaterial3d(material.clone()));
                    }
                }
                None => {
                    if textured.is_some() {
                        let handle = fallback
                            .get_or_insert_with(|| {
                                let handle = new_terrain_chunk_material(config, &mut standard_materials);
                                commands.entity(root).try_insert(TerrainChunkMaterial(handle.clone()));
                                handle
                            })
                            .clone();
                        commands
                            .entity(child)
                            .try_remove::<MeshMaterial3d<TerrainSurfaceMaterial>>()
                            .try_insert(MeshMaterial3d(handle));
                    }
                }
            }
        }
    }
}

/// Give every terrain root a [`TerrainHeightTexture`] while one is wanted and
/// keep it current (see the module docs); remove it when it is not wanted or
/// the root has no raster to draw it from. Runs after
/// `apply_terrain_dirty_chunks`, so the height stamps of this frame's edits
/// and bakes are already there.
pub fn sync_terrain_height_textures(
    mut commands: Commands,
    time: Option<Res<Time<Real>>>,
    request: Option<Res<TerrainHeightTextureRequest>>,
    dirty: Option<Res<TerrainDirtyChunks>>,
    mut images: ResMut<Assets<Image>>,
    mut roots: Query<
        (Entity, Ref<TerrainConfig>, &TerrainData, Option<&TerrainBaked>, Option<&mut TerrainHeightTexture>),
        With<TerrainRoot>,
    >,
) {
    let wanted = request.is_some_and(|request| request.wanted);
    let now = time.map(|time| time.elapsed_secs_f64());
    // Heights only: paint and volume marks leave this, and a texture of
    // heights has nothing to re-upload for them.
    let seq = dirty.as_ref().map_or(0, |dirty| dirty.height_seq());
    for (entity, config, base, baked, texture) in &mut roots {
        let data = surface_data(base, baked);
        let Some(size) = height_texture_size(data).filter(|_| wanted) else {
            if texture.is_some() {
                commands.entity(entity).try_remove::<TerrainHeightTexture>();
            }
            continue;
        };
        let cache_id = data.height_cache.as_ptr() as usize;
        match texture {
            Some(mut texture) if texture.size == size => {
                texture.age_frames = texture.age_frames.saturating_add(1);
                if texture.uploaded_cache != cache_id || texture.uploaded_seq != seq || config.is_changed() {
                    texture.stale = true;
                }
                let due = match (now, texture.uploaded_at) {
                    (Some(now), Some(at)) => now - at >= HEIGHT_UPLOAD_INTERVAL_SECS,
                    _ => true,
                };
                if texture.stale && due {
                    // Same size, so Bevy writes into the existing texture and
                    // the water material's bind group stays valid.
                    if let Some(mut image) = images.get_mut(&texture.image) {
                        image.data = Some(height_map_bytes(&config, data));
                    }
                    texture.uploaded_cache = cache_id;
                    texture.uploaded_seq = seq;
                    texture.uploaded_at = now;
                    texture.stale = false;
                }
            }
            _ => {
                // First texture, or the raster changed size: a new image,
                // which waits out its settle frames before it is bound.
                let image = images.add(height_map_image(&config, data, size));
                commands.entity(entity).try_insert(TerrainHeightTexture {
                    image,
                    size,
                    uploaded_cache: cache_id,
                    uploaded_seq: seq,
                    uploaded_at: now,
                    stale: false,
                    age_frames: 0,
                });
            }
        }
    }
}

/// The textured terrain material: its shader, its `MaterialPlugin` and the
/// systems above. Added by the shared `TerrainPlugin` (Client) and by the
/// engine's `EngineTerrainPlugin`, each guarding against adding it twice.
pub struct TerrainSurfacePlugin;

impl Plugin for TerrainSurfacePlugin {
    fn build(&self, app: &mut App) {
        // The slot table and texture arrays this material draws from.
        if !app.is_plugin_added::<TerrainMaterialSlotsPlugin>() {
            app.add_plugins(TerrainMaterialSlotsPlugin);
        }
        // A material plugin sets up its render side in `build`, so the render
        // sub-app must exist by now: add this after the render plugins
        // (DefaultPlugins), as both hosts do. Without one, chunks keep the
        // vertex-colour material.
        if app.get_sub_app(bevy::render::RenderApp).is_none() {
            debug!(target: "eustress::terrain::textures", "no renderer: terrain chunks keep the vertex-colour material");
            return;
        }
        embedded_asset!(app, "terrain_surface.wgsl");
        app.add_plugins(MaterialPlugin::<TerrainSurfaceMaterial>::default())
            .init_resource::<TerrainSurfaceBindings>()
            .init_resource::<TerrainHeightTextureRequest>()
            .init_resource::<TerrainDirtyChunks>()
            // After the dirty-chunk pass, so a stroke or bake of this frame
            // is uploaded from its finished heights.
            .add_systems(Update, sync_terrain_height_textures.after(super::apply_terrain_dirty_chunks))
            // After the arrays publish and after every system that spawns
            // or remeshes chunks, so the swap sees this frame's chunks.
            .add_systems(
                Update,
                (settle_terrain_surface_bindings, sync_terrain_surfaces, swap_terrain_chunk_materials)
                    .chain()
                    .after(super::texture_arrays::drive_terrain_texture_arrays)
                    .after(super::process_terrain_generation_queue)
                    .after(super::chunk_spawn_system)
                    .after(super::apply_terrain_dirty_chunks),
            );
    }
}

/// Reading a struct's layout out of WGSL source, so the tests of every
/// embedded shader can hold its structs to their Rust packing.
#[cfg(test)]
pub(crate) mod wgsl_layout {
    /// The `(field, type)` list of WGSL struct `name` in `shader`.
    pub fn struct_fields(shader: &str, name: &str) -> Vec<(String, String)> {
        let start = shader.find(&format!("struct {name} {{")).unwrap_or_else(|| panic!("struct {name} is in the shader"));
        let body = &shader[start..];
        let (open, close) = (body.find('{').unwrap(), body.find('}').unwrap());
        body[open + 1..close]
            .lines()
            .filter_map(|line| {
                let line = line.split("//").next().unwrap_or("").trim().trim_end_matches(',');
                let (field, ty) = line.split_once(':')?;
                Some((field.trim().to_string(), ty.trim().to_string()))
            })
            .collect()
    }

    /// Alignment and size of the WGSL types these structs use (WGSL spec,
    /// "Alignment and Size"; the same in the uniform and storage address
    /// spaces for scalars and vectors).
    pub fn align_size(ty: &str) -> (usize, usize) {
        match ty {
            "f32" | "i32" | "u32" => (4, 4),
            "vec2<f32>" | "vec2<i32>" | "vec2<u32>" => (8, 8),
            "vec3<f32>" | "vec3<i32>" | "vec3<u32>" => (16, 12),
            "vec4<f32>" | "vec4<i32>" | "vec4<u32>" => (16, 16),
            other => panic!("no layout rule for WGSL type {other}"),
        }
    }

    /// Member offsets, size and alignment of a WGSL struct with `fields`.
    pub fn layout(fields: &[(String, String)]) -> (Vec<(String, usize)>, usize, usize) {
        let (mut offset, mut align) = (0usize, 1usize);
        let mut members = Vec::new();
        for (name, ty) in fields {
            let (a, s) = align_size(ty);
            offset = offset.next_multiple_of(a);
            members.push((name.clone(), offset));
            offset += s;
            align = align.max(a);
        }
        (members, offset.next_multiple_of(align), align)
    }

    pub fn offset_in(members: &[(String, usize)], name: &str) -> usize {
        members.iter().find(|(member, _)| member == name).unwrap_or_else(|| panic!("no member {name}")).1
    }

    pub fn f32_at(bytes: &[u8], at: usize) -> f32 {
        f32::from_ne_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    pub fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_ne_bytes(bytes[at..at + 4].try_into().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::wgsl_layout::{f32_at, layout as wgsl_layout, offset_in, u32_at};
    use super::*;
    use crate::terrain::height_query::{ensure_material_cache, height_at_world, world_to_uv};
    use crate::terrain::layers::{FlattenPadLayer, LayerDesc, LayerKind};
    use crate::terrain::material::{canonical_material_cell, material_cell, TerrainMaterial};
    use crate::terrain::texture_arrays::TerrainTextureLayer;
    use crate::terrain::TerrainRebake;
    use bevy::render::render_resource::encase::UniformBuffer;
    use std::mem::{offset_of, size_of};

    const SHADER: &str = include_str!("terrain_surface.wgsl");

    /// The `(field, type)` list of WGSL struct `name` in [`SHADER`].
    fn wgsl_struct_fields(name: &str) -> Vec<(String, String)> {
        super::wgsl_layout::struct_fields(SHADER, name)
    }

    #[test]
    fn slot_record_layout_matches_the_wgsl_struct() {
        let (members, size, align) = wgsl_layout(&wgsl_struct_fields("TerrainSlotRecord"));
        let pad = offset_of!(TerrainSlotRecord, _pad);
        let rust = [
            ("albedo", offset_of!(TerrainSlotRecord, albedo)),
            ("layer", offset_of!(TerrainSlotRecord, layer)),
            ("repeats_per_metre", offset_of!(TerrainSlotRecord, repeats_per_metre)),
            ("roughness", offset_of!(TerrainSlotRecord, roughness)),
            ("roughness_scale", offset_of!(TerrainSlotRecord, roughness_scale)),
            ("metallic", offset_of!(TerrainSlotRecord, metallic)),
            ("height_scale", offset_of!(TerrainSlotRecord, height_scale)),
            ("_pad0", pad),
            ("_pad1", pad + 4),
            ("_pad2", pad + 8),
        ];
        assert_eq!(members.len(), rust.len(), "the shader's fields: {members:?}");
        for ((wgsl_name, wgsl_offset), (rust_name, rust_offset)) in members.iter().zip(rust) {
            assert_eq!(wgsl_name, rust_name, "fields in the same order");
            assert_eq!(*wgsl_offset, rust_offset, "{rust_name} sits where the shader reads it");
        }
        assert_eq!(size, 48);
        assert_eq!(size, size_of::<TerrainSlotRecord>(), "the WGSL size is the Rust size");
        assert_eq!(size % align, 0, "so the array stride is the size, as in a Rust slice");

        // The bytes the buffer receives: the second record starts one stride
        // in, and each field decodes where the shader reads it.
        let record = TerrainSlotRecord {
            albedo: [0.25, 0.5, 0.75],
            layer: 7,
            repeats_per_metre: 0.125,
            roughness: 0.6,
            roughness_scale: 1.5,
            metallic: 0.2,
            height_scale: 2.0,
            _pad: [0.0; 3],
        };
        let records = [TerrainSlotRecord::default(), record];
        let bytes: &[u8] = bytemuck::cast_slice(&records);
        assert_eq!(bytes.len(), 2 * size);
        let at = |name: &str| size + offset_in(&members, name);
        assert_eq!(f32_at(bytes, at("albedo") + 8), 0.75);
        assert_eq!(i32::from_ne_bytes(bytes[at("layer")..at("layer") + 4].try_into().unwrap()), 7);
        assert_eq!(f32_at(bytes, at("repeats_per_metre")), 0.125);
        assert_eq!(f32_at(bytes, at("roughness_scale")), 1.5);
        assert_eq!(f32_at(bytes, at("height_scale")), 2.0);
        assert!(SHADER.contains("array<TerrainSlotRecord, 256>"), "the shader indexes all 256 slots");
        assert_eq!(MATERIAL_SLOT_COUNT, 256);
    }

    #[test]
    fn surface_params_layout_matches_the_wgsl_struct() {
        let (members, size, _) = wgsl_layout(&wgsl_struct_fields("TerrainSurfaceParams"));
        assert_eq!(
            members.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(),
            ["world_origin", "world_extent", "cache_size", "flags", "blend_depth"]
        );
        assert_eq!(TerrainSurfaceParams::min_size().get() as usize, size, "encase and the shader agree on the size");

        let params = TerrainSurfaceParams {
            world_origin: Vec2::new(-1.5, -2.5),
            world_extent: Vec2::new(3.5, 4.5),
            cache_size: UVec2::new(5, 6),
            flags: 7,
            blend_depth: 0.25,
        };
        let mut buffer = UniformBuffer::new(Vec::<u8>::new());
        buffer.write(&params).unwrap();
        let bytes = buffer.into_inner();
        assert!(bytes.len() >= size);
        let at = |name: &str| offset_in(&members, name);
        assert_eq!(f32_at(&bytes, at("world_origin") + 4), -2.5);
        assert_eq!(f32_at(&bytes, at("world_extent")), 3.5);
        assert_eq!(u32_at(&bytes, at("cache_size") + 4), 6);
        assert_eq!(u32_at(&bytes, at("flags")), 7);
        assert_eq!(f32_at(&bytes, at("blend_depth")), 0.25);
    }

    #[test]
    fn the_raster_mapping_is_the_one_sample_height_uses() {
        let config = TerrainConfig { chunk_size: 48.0, chunk_resolution: 8, chunks_x: 3, chunks_z: 2, ..TerrainConfig::default() };
        let size = UVec2::new((config.chunks_x * 2 + 1) * config.chunk_resolution, (config.chunks_z * 2 + 1) * config.chunk_resolution);
        let params = TerrainSurfaceParams::new(&config, size, true);
        assert_eq!(params.flags, TERRAIN_SURFACE_FLAG_TEXTURED);
        for (x, z) in [(0.0, 0.0), (-144.0, -96.0), (192.0, 144.0), (13.7, -55.25), (-900.0, 900.0), (47.9, 0.1)] {
            let (u, v) = world_to_uv(&config, x, z);
            let expected = Vec2::new(u * (size.x - 1) as f32, v * (size.y - 1) as f32);
            let got = params.raster_texel(x, z);
            assert!((got - expected).abs().max_element() < 1e-3, "({x}, {z}): {got} against {expected}");
        }
        assert_eq!(TerrainSurfaceParams::new(&config, size, false).flags, 0);
    }

    #[test]
    fn the_material_map_is_the_cells_in_raster_order() {
        let config = TerrainConfig { chunk_resolution: 4, chunks_x: 1, chunks_z: 1, ..TerrainConfig::default() };
        let mut data = TerrainData::procedural();
        assert_eq!(surface_raster_size(&data), None, "procedural terrain keeps the vertex colours");
        data.resize_cache(&config);
        assert_eq!(surface_raster_size(&data), None, "a raster without a material layer too");
        ensure_material_cache(&mut data);
        assert_eq!(surface_raster_size(&data), Some(UVec2::new(12, 12)));

        let rock = TerrainMaterial::Rock.to_u8();
        data.material_cache[13] = canonical_material_cell(rock, 200, 40);
        let bytes = material_map_bytes(&data);
        assert_eq!(bytes.len(), 12 * 12 * 4);
        assert_eq!(&bytes[13 * 4..14 * 4], &[rock, 200, 40, 0]);
        assert_eq!(&bytes[..4], &material_cell(TerrainMaterial::Grass.to_u8()));

        // Wider than a texture may be: vertex colours.
        let mut wide = TerrainData::procedural();
        wide.cache_width = MAX_MATERIAL_MAP_SIDE + 1;
        wide.cache_height = 1;
        wide.height_cache = vec![0.0; wide.cache_width as usize];
        ensure_material_cache(&mut wide);
        assert!(wide.has_material_layer());
        assert_eq!(surface_raster_size(&wide), None);
    }

    #[test]
    fn slot_records_draw_flat_without_arrays_and_from_their_layer_with_them() {
        let slots = TerrainMaterialSlots::builtins();
        let flat = pack_slot_records(&slots, None);
        assert_eq!(flat.len(), MATERIAL_SLOT_COUNT);
        let grass = slots.get(TerrainMaterial::Grass.to_u8()).unwrap();
        let grass_flat = flat[TerrainMaterial::Grass as usize];
        assert_eq!(grass_flat.layer, -1);
        assert_eq!(grass_flat.albedo, grass.swatch_linear(), "a flat slot draws its swatch");
        assert_eq!(grass_flat.height_scale, 0.0);
        assert!((grass_flat.repeats_per_metre - 1.0 / grass.tiling).abs() < 1e-6);
        assert!(flat.iter().all(|record| record.layer == -1));
        assert_eq!(bytemuck::cast_slice::<TerrainSlotRecord, u8>(&flat).len(), MATERIAL_SLOT_COUNT * 48);

        // Arrays holding every built-in set: slots sharing a set share its layer.
        let layers: Vec<TerrainTextureLayer> = slots
            .unique_texture_sets()
            .into_iter()
            .map(|set| TerrainTextureLayer {
                set,
                loaded: true,
                mean_albedo_linear: [0.2, 0.3, 0.1],
                mean_occlusion: 1.0,
                mean_roughness: 0.5,
                mean_metallic: 0.0,
            })
            .collect();
        let arrays = TerrainTextureArraySet {
            albedo: Handle::default(),
            normal: Handle::default(),
            orm: Handle::default(),
            layers,
            resolution: 4,
            mip_levels: 3,
        };
        let textured = pack_slot_records(&slots, Some(&arrays));
        let grass_textured = textured[TerrainMaterial::Grass as usize];
        assert_eq!(grass_textured.layer, 0);
        assert_eq!(grass_textured.albedo, grass.tint, "a textured slot tints its layer");
        assert!((grass_textured.roughness_scale - grass.roughness / 0.5).abs() < 1e-6);
        let luminance = 0.2126 * 0.2 + 0.7152 * 0.3 + 0.0722 * 0.1;
        assert!((grass_textured.height_scale - 0.5 / luminance).abs() < 1e-4);
        assert_eq!(textured[TerrainMaterial::LeafyGrass as usize].layer, 0);
        assert_eq!(textured[TerrainMaterial::Mud as usize].layer, textured[TerrainMaterial::Sand as usize].layer);
        // Undefined slots stay flat.
        assert_eq!(textured[200].layer, -1);
    }

    #[test]
    fn height_texture_bytes_are_the_surface_heights_in_raster_order() {
        let config = TerrainConfig {
            chunk_size: 32.0,
            chunk_resolution: 8,
            chunks_x: 1,
            chunks_z: 1,
            height_scale: 40.0,
            height_offset: -10.0,
            ..TerrainConfig::default()
        };
        let mut base = TerrainData::procedural();
        assert_eq!(height_texture_size(&base), None, "procedural terrain has no raster to upload");
        base.resize_cache(&config);
        let width = base.cache_width as usize;
        for (i, sample) in base.height_cache.iter_mut().enumerate() {
            *sample = ((i % width) as f32 * 0.37 + (i / width) as f32 * 0.11).fract();
        }
        assert_eq!(height_texture_size(&base), Some(UVec2::new(24, 24)));

        // A pad baked over the base: the texture holds the SURFACE.
        let pad = LayerDesc {
            id: 1,
            order: 0,
            kind: LayerKind::FlattenPad(FlattenPadLayer {
                center: Vec3::new(8.0, 20.0, 8.0),
                yaw: 0.0,
                size: Vec2::splat(10.0),
                falloff: 2.0,
                material: None,
            }),
        };
        let mut baked = TerrainBaked::new(&base, vec![pad]);
        baked.rebake(&config, &base, &TerrainRebake::default());
        let surface = surface_data(&base, Some(&baked));
        let bytes = height_map_bytes(&config, surface);
        assert_eq!(bytes.len(), 24 * 24 * 4, "one f32 per cell");
        let texel = |x: usize, z: usize| f32_at(&bytes, (z * width + x) * 4);
        for (i, sample) in surface.height_cache.iter().enumerate() {
            let expected = config.world_height(*sample);
            assert_eq!(f32_at(&bytes, i * 4), expected, "cell {i} is its world height");
        }

        // The texel a shader reads at a cell's centre, through the material
        // map's raster mapping, is the height the CPU reads there.
        let params = TerrainSurfaceParams::new(&config, UVec2::new(24, 24), false);
        let (min, max) = config.footprint_xz();
        let step = (max - min) / 23.0;
        for (x, z) in [(0usize, 0usize), (5, 17), (11, 11), (23, 23), (12, 9)] {
            let world = min + Vec2::new(x as f32, z as f32) * step;
            let at = params.raster_texel(world.x, world.y);
            assert!((at - Vec2::new(x as f32, z as f32)).abs().max_element() < 1e-3, "({x}, {z}) maps to texel {at}");
            let cpu = height_at_world(&config, surface, world.x, world.y);
            assert!((texel(x, z) - cpu).abs() < 1e-3, "({x}, {z}): texel {} against the surface's {cpu}", texel(x, z));
        }
        // The pad is in the texture, and it is not in the base.
        let under_pad = height_at_world(&config, surface, 8.0, 8.0);
        assert!((under_pad - 20.0).abs() < 1e-3, "the baked pad reads {under_pad}");
        assert!((height_at_world(&config, &base, 8.0, 8.0) - 20.0).abs() > 1e-3);

        // A raster wider than a texture may be uploads nothing.
        let mut wide = TerrainData::procedural();
        wide.cache_width = MAX_MATERIAL_MAP_SIDE + 1;
        wide.cache_height = 1;
        wide.height_cache = vec![0.0; wide.cache_width as usize];
        assert_eq!(height_texture_size(&wide), None);
    }
}
