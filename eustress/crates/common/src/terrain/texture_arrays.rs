//! Terrain texture arrays: the albedo, normal and ORM `texture_2d_array`s the
//! textured terrain material samples.
//!
//! Every distinct [`TerrainTextureSet`] of the slot table
//! (`TerrainMaterialSlots::unique_texture_sets`) is one layer, so the five
//! built-ins drawn with sand share a single sand layer. The built-ins alone
//! make nine layers.
//!
//! ## Building
//!
//! [`drive_terrain_texture_arrays`] starts a build on `AsyncComputeTaskPool`
//! when a terrain that can be drawn textured exists (a height raster with a
//! material layer) and the arrays do not match the slot table's sets and the
//! [`TerrainTextureSettings`] resolution. The sets decode one after another
//! on that single task, one map at a time so the build never holds more
//! than one full-size decode: a square power-of-two source at least the
//! layer size (every bundled 2048x2048 map) is box-filtered down the
//! texture-gen mip chain, anything else is resized first; then the layer's
//! CPU mip chain runs down to 1x1. The finished layers are packed
//! layer-major (each layer's mips in turn, the order wgpu reads array data
//! in) and handed back; the main thread only wraps the three buffers in
//! `Image`s. The images are
//! render-world only, so the CPU copy is freed once uploaded.
//!
//! A set whose albedo cannot be read becomes a white layer marked not
//! loaded, and the slots on it draw flat in their swatch colour (see
//! [`TerrainTextureArrays::slot_surface`]); a missing normal or ORM map is
//! flat. A build fails only when no set loads at all.
//!
//! ## Using them
//!
//! [`TerrainTextureArrays::current`] holds the arrays to bind, and stays on
//! the previous arrays while a rebuild runs, so a slot added mid-session
//! draws flat until its layer exists rather than the whole terrain dropping
//! back to vertex colours. [`TerrainTextureArrays::generation`] changes
//! whenever `current` does. With no arrays (building the first time, failed,
//! disabled, or no renderer) the chunks keep the vertex-colour material.
//!
//! The arrays are released once no such terrain has existed for
//! [`RELEASE_GRACE_SECS`], which rides out the gap between a Space switch
//! despawning one terrain and spawning the next.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDataOrder, TextureDimension, TextureFormat, TextureViewDescriptor,
    TextureViewDimension,
};
use bevy::tasks::{block_on, poll_once, Task};
use texture_gen::mips::{build_mip_chain_rgba8, mip_level_count, srgb_to_linear, MapKind};
// Explicit, so the log macros do not depend on the prelude's `bevy_log`
// feature (see `avatar::boot`).
use tracing::{info, warn};

use super::material_slots::{TerrainMaterialSlots, TerrainTextureSet, UNDEFINED_SLOT_SRGB};
use super::surface_material::surface_raster_size;
use super::{surface_data, TerrainBaked, TerrainData, TerrainRoot};

/// Seconds without any terrain before the arrays are released.
pub const RELEASE_GRACE_SECS: f64 = 10.0;

/// Texel a missing normal map reads as: straight up.
const FLAT_NORMAL_TEXEL: [u8; 4] = [128, 128, 255, 255];
/// Texel a missing ORM map reads as: no occlusion, roughness 1, metallic 0.
/// With a mean roughness of 1 the slot's own roughness is used as is.
const DEFAULT_ORM_TEXEL: [u8; 4] = [255, 255, 0, 255];
/// Albedo of a layer whose image could not be read.
const MISSING_ALBEDO_TEXEL: [u8; 4] = [255, 255, 255, 255];
/// Floor under a layer's mean roughness when scaling it to a slot's, so a
/// near-mirror texture does not turn a small roughness into a huge scale.
const MIN_MEAN_ROUGHNESS: f32 = 0.02;

/// Side of one texture-array layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TerrainTextureResolution {
    /// 512x512: a quarter of the memory, for low-end GPUs.
    R512,
    /// 1024x1024.
    #[default]
    R1024,
}

impl TerrainTextureResolution {
    /// Texels per side.
    pub const fn pixels(self) -> u32 {
        match self {
            Self::R512 => 512,
            Self::R1024 => 1024,
        }
    }
}

/// How the terrain texture arrays are built. Changing either field rebuilds
/// or releases them.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct TerrainTextureSettings {
    /// Side of every layer.
    pub layer_resolution: TerrainTextureResolution,
    /// Build the arrays at all; `false` keeps the vertex-colour terrain.
    pub enabled: bool,
}

impl Default for TerrainTextureSettings {
    fn default() -> Self {
        Self { layer_resolution: TerrainTextureResolution::default(), enabled: true }
    }
}

/// Where the latest build stands.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum TerrainTextureStatus {
    /// Nothing built or wanted: no terrain that can be drawn textured, or
    /// disabled.
    #[default]
    Idle,
    /// A build is running.
    Building,
    /// The latest build finished; `current` holds it.
    Ready,
    /// The latest build failed. `current` keeps any earlier arrays.
    Failed(String),
    /// This app cannot build them (no renderer, or no image decoder).
    Unavailable(String),
}

/// One layer of the arrays and what it measured.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainTextureLayer {
    /// The set this layer holds.
    pub set: TerrainTextureSet,
    /// The albedo decoded; `false` for a white placeholder layer.
    pub loaded: bool,
    /// Mean linear albedo (the 1x1 mip).
    pub mean_albedo_linear: [f32; 3],
    /// Mean of the ORM channels: occlusion, roughness, metallic.
    pub mean_occlusion: f32,
    pub mean_roughness: f32,
    pub mean_metallic: f32,
}

/// Built arrays, ready to bind.
#[derive(Clone, Debug)]
pub struct TerrainTextureArraySet {
    /// sRGB albedo, `Rgba8UnormSrgb`.
    pub albedo: Handle<Image>,
    /// Tangent-space normals, `Rgba8Unorm`.
    pub normal: Handle<Image>,
    /// Occlusion, roughness, metallic in R, G, B, `Rgba8Unorm`.
    pub orm: Handle<Image>,
    /// The layers, in array order.
    pub layers: Vec<TerrainTextureLayer>,
    /// Texels per layer side.
    pub resolution: u32,
    /// Mip levels per layer, down to 1x1.
    pub mip_levels: u32,
}

impl TerrainTextureArraySet {
    /// Array layer holding `set`, `None` when it has none or its albedo did
    /// not load.
    pub fn layer_of(&self, set: &TerrainTextureSet) -> Option<u32> {
        self.layers
            .iter()
            .position(|layer| layer.loaded && layer.set == *set)
            .and_then(|index| u32::try_from(index).ok())
    }
}

/// How the textured terrain material draws one slot: the record behind the
/// shader's per-slot table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainSlotSurface {
    /// Array layer to sample; `None` draws the slot flat in `albedo`.
    pub layer: Option<u32>,
    /// Linear RGB: the multiplier on the layer's albedo (the slot's tint),
    /// or the flat colour (its swatch) when `layer` is `None`.
    pub albedo: [f32; 3],
    /// Metres of ground per texture repeat.
    pub tiling: f32,
    /// The slot's perceptual roughness, what a flat slot uses as is.
    pub roughness: f32,
    /// Multiplier on the ORM roughness channel that brings the layer's mean
    /// to `roughness`; 1 for a flat slot.
    pub roughness_scale: f32,
    /// Metallic, uniform over the slot.
    pub metallic: f32,
}

/// The terrain texture arrays and the state of their latest build. See the
/// module docs.
#[derive(Resource, Debug, Default)]
pub struct TerrainTextureArrays {
    status: TerrainTextureStatus,
    current: Option<TerrainTextureArraySet>,
    generation: u64,
    rebuild_requested: bool,
}

impl TerrainTextureArrays {
    /// Where the latest build stands.
    pub fn status(&self) -> &TerrainTextureStatus {
        &self.status
    }

    /// The arrays to bind, if any exist.
    pub fn current(&self) -> Option<&TerrainTextureArraySet> {
        self.current.as_ref()
    }

    /// Changes every time [`Self::current`] does.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Rebuild from the files again, after an image a slot reads changed.
    pub fn request_rebuild(&mut self) {
        self.rebuild_requested = true;
    }

    /// Array layer slot `slot` samples, `None` when it draws flat.
    pub fn slot_layer(&self, slots: &TerrainMaterialSlots, slot: u8) -> Option<u32> {
        slot_layer_in(slots, self.current.as_ref(), slot)
    }

    /// How to draw slot `slot` with the current arrays. An undefined slot is
    /// flat neutral grey, the colour the vertex-colour mesher gives it.
    pub fn slot_surface(&self, slots: &TerrainMaterialSlots, slot: u8) -> TerrainSlotSurface {
        slot_surface_in(slots, self.current.as_ref(), slot)
    }

    /// Drop the arrays.
    fn release(&mut self) {
        if self.current.take().is_some() {
            self.generation = self.generation.wrapping_add(1);
        }
        self.status = TerrainTextureStatus::Idle;
    }
}

/// Array layer slot `slot` samples in `arrays`, `None` when it draws flat
/// there. [`TerrainTextureArrays::slot_layer`] asks this of the current
/// arrays; the surface material asks it of the arrays it has bound, which
/// trail the current ones by a few frames (see `surface_material`).
pub fn slot_layer_in(slots: &TerrainMaterialSlots, arrays: Option<&TerrainTextureArraySet>, slot: u8) -> Option<u32> {
    let set = slots.get(slot)?.texture_set.as_ref()?;
    arrays?.layer_of(set)
}

/// How to draw slot `slot` with `arrays` (see [`slot_layer_in`]). An
/// undefined slot is flat neutral grey, the colour the vertex-colour mesher
/// gives it.
pub fn slot_surface_in(slots: &TerrainMaterialSlots, arrays: Option<&TerrainTextureArraySet>, slot: u8) -> TerrainSlotSurface {
    let Some(def) = slots.get(slot) else {
        let [r, g, b] = UNDEFINED_SLOT_SRGB;
        let linear = Color::srgb(r, g, b).to_linear();
        return TerrainSlotSurface {
            layer: None,
            albedo: [linear.red, linear.green, linear.blue],
            tiling: 4.0,
            roughness: 0.9,
            roughness_scale: 1.0,
            metallic: 0.0,
        };
    };
    let layer = slot_layer_in(slots, arrays, slot);
    let mean_roughness = layer
        .and_then(|index| arrays?.layers.get(index as usize))
        .map(|layer| layer.mean_roughness);
    match (layer, mean_roughness) {
        (Some(index), Some(mean)) => TerrainSlotSurface {
            layer: Some(index),
            albedo: def.tint,
            tiling: def.tiling,
            roughness: def.roughness,
            roughness_scale: def.roughness / mean.max(MIN_MEAN_ROUGHNESS),
            metallic: def.metallic,
        },
        _ => TerrainSlotSurface {
            layer: None,
            albedo: def.swatch_linear(),
            tiling: def.tiling,
            roughness: def.roughness,
            roughness_scale: 1.0,
            metallic: def.metallic,
        },
    }
}

/// What a build is for: the sets in layer order, at one resolution.
#[derive(Clone, Debug, PartialEq)]
struct BuildKey {
    sets: Vec<TerrainTextureSet>,
    resolution: u32,
}

/// Arrays a build task produced, not yet wrapped in images.
struct BuiltArrays {
    resolution: u32,
    mip_levels: u32,
    albedo: Vec<u8>,
    normal: Vec<u8>,
    orm: Vec<u8>,
    layers: Vec<TerrainTextureLayer>,
}

struct PendingBuild {
    key: BuildKey,
    /// Dropping it cancels the build.
    task: Task<Result<BuiltArrays, String>>,
}

/// Bookkeeping behind [`drive_terrain_texture_arrays`]: the running build
/// task and what the published arrays were built from. Internal; read
/// [`TerrainTextureArrays`] instead.
#[derive(Resource, Default)]
pub struct TerrainTextureBuildState {
    /// The key the slot table and settings currently ask for, recomputed
    /// only when either changes.
    wanted: Option<BuildKey>,
    pending: Option<PendingBuild>,
    /// The key of the arrays in `current`, or of the build that last failed,
    /// so an unchanged request neither rebuilds nor retries a failure.
    settled: Option<BuildKey>,
    /// `Time::elapsed_secs_f64` of the last frame a terrain that can wear the
    /// textured surface (see `surface_material::surface_raster_size`)
    /// existed. `None` until one does, so an app that never shows such a
    /// terrain never builds.
    terrain_last_seen: Option<f64>,
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<TerrainTextureSettings>()
        .init_resource::<TerrainTextureArrays>()
        .init_resource::<TerrainTextureBuildState>();
}

/// After every plugin has built: without a render sub-app nothing would ever
/// upload the arrays, so they are never built.
pub(crate) fn finish(app: &mut App) {
    if app.get_sub_app(bevy::render::RenderApp).is_none() {
        app.world_mut().resource_mut::<TerrainTextureArrays>().status =
            TerrainTextureStatus::Unavailable("no renderer".to_string());
    }
}

/// Start, poll, publish and release the terrain texture arrays. See the
/// module docs. Touches [`TerrainTextureArrays`] mutably only on a real
/// transition, so its change detection means "something to rebind".
pub fn drive_terrain_texture_arrays(
    settings: Res<TerrainTextureSettings>,
    slots: Res<TerrainMaterialSlots>,
    mut arrays: ResMut<TerrainTextureArrays>,
    mut build: ResMut<TerrainTextureBuildState>,
    images: Option<ResMut<Assets<Image>>>,
    roots: Query<(&TerrainData, Option<&TerrainBaked>), With<TerrainRoot>>,
    time: Res<Time>,
) {
    let Some(mut images) = images else {
        return;
    };
    if matches!(arrays.status, TerrainTextureStatus::Unavailable(_)) {
        return;
    }
    let build = &mut *build;

    let now = time.elapsed_secs_f64();
    // Only a terrain that can wear the surface counts: procedural terrain
    // (every Client terrain today) and a raster with no material layer keep
    // the vertex-colour material, and nothing would ever bind arrays
    // decoded for them. Same eligibility as `sync_terrain_surfaces`, which
    // draws a root's layer bake when it has one.
    if roots.iter().any(|(data, baked)| surface_raster_size(surface_data(data, baked)).is_some()) {
        build.terrain_last_seen = Some(now);
    }
    let terrain_wanted = build
        .terrain_last_seen
        .is_some_and(|seen| now - seen < RELEASE_GRACE_SECS);

    if !settings.enabled || !terrain_wanted {
        build.pending = None;
        build.settled = None;
        // Recomputed on the way back: slot changes seen while idle do not
        // show up in `is_changed` later.
        build.wanted = None;
        if arrays.current.is_some() || arrays.status != TerrainTextureStatus::Idle {
            arrays.release();
        }
        return;
    }

    if build.wanted.is_none() || slots.is_changed() || settings.is_changed() {
        build.wanted = Some(BuildKey {
            sets: slots.unique_texture_sets(),
            resolution: settings.layer_resolution.pixels(),
        });
    }
    let Some(wanted) = build.wanted.clone() else {
        return;
    };

    if arrays.rebuild_requested {
        arrays.rebuild_requested = false;
        build.settled = None;
        build.pending = None;
    }

    if build.pending.as_ref().is_some_and(|pending| pending.key != wanted) {
        // Superseded: dropping the task cancels it.
        build.pending = None;
    }
    if let Some(pending) = build.pending.as_mut() {
        let Some(result) = block_on(poll_once(&mut pending.task)) else {
            return;
        };
        let key = pending.key.clone();
        build.pending = None;
        match result {
            Ok(built) => {
                let layer_count = built.layers.len();
                let missing: Vec<String> =
                    built.layers.iter().filter(|layer| !layer.loaded).map(|layer| layer.set.label()).collect();
                let bytes = built.albedo.len() + built.normal.len() + built.orm.len();
                let set = publish(built, &mut images);
                info!(
                    target: "eustress::terrain::textures",
                    layers = layer_count,
                    resolution = set.resolution,
                    megabytes = bytes / (1024 * 1024),
                    "terrain texture arrays ready"
                );
                if !missing.is_empty() {
                    warn!(
                        target: "eustress::terrain::textures",
                        "terrain texture sets that did not load draw flat: {}",
                        missing.join(", ")
                    );
                }
                arrays.current = Some(set);
                arrays.generation = arrays.generation.wrapping_add(1);
                arrays.status = TerrainTextureStatus::Ready;
            }
            Err(error) => {
                warn!(target: "eustress::terrain::textures", "terrain texture arrays failed: {error}");
                arrays.status = TerrainTextureStatus::Failed(error);
            }
        }
        build.settled = Some(key);
        return;
    }

    if build.settled.as_ref() == Some(&wanted) {
        return;
    }
    start_build(wanted, build, &mut arrays);
}

/// Spawn the build for `key`.
#[cfg(feature = "image")]
fn start_build(key: BuildKey, build: &mut TerrainTextureBuildState, arrays: &mut TerrainTextureArrays) {
    let task = spawn_build(key.clone(), crate::avatar::boot::bundled_root());
    build.pending = Some(PendingBuild { key, task });
    arrays.status = TerrainTextureStatus::Building;
}

/// Without the `image` feature there is no decoder: say so once and stay on
/// the vertex-colour terrain.
#[cfg(not(feature = "image"))]
fn start_build(key: BuildKey, build: &mut TerrainTextureBuildState, arrays: &mut TerrainTextureArrays) {
    build.settled = Some(key);
    arrays.status =
        TerrainTextureStatus::Unavailable("built without the `image` feature, which decodes the textures".to_string());
}

/// Wrap `built` in three array images.
fn publish(built: BuiltArrays, images: &mut Assets<Image>) -> TerrainTextureArraySet {
    let layers = u32::try_from(built.layers.len()).unwrap_or(u32::MAX);
    let (resolution, mip_levels) = (built.resolution, built.mip_levels);
    let image = |data: Vec<u8>, format: TextureFormat| array_image(data, resolution, layers, mip_levels, format);
    TerrainTextureArraySet {
        albedo: images.add(image(built.albedo, TextureFormat::Rgba8UnormSrgb)),
        normal: images.add(image(built.normal, TextureFormat::Rgba8Unorm)),
        orm: images.add(image(built.orm, TextureFormat::Rgba8Unorm)),
        layers: built.layers,
        resolution,
        mip_levels,
    }
}

/// A `resolution`-square, `layers`-deep 2D array image with `mip_levels`
/// mips, `data` packed layer-major. `Image::new` would assert the data holds
/// one level only, hence `new_uninit`. Repeat addressing for world-space
/// tiling, trilinear with anisotropy for ground seen at grazing angles.
fn array_image(data: Vec<u8>, resolution: u32, layers: u32, mip_levels: u32, format: TextureFormat) -> Image {
    let mut image = Image::new_uninit(
        Extent3d { width: resolution, height: resolution, depth_or_array_layers: layers },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.data_order = TextureDataOrder::LayerMajor;
    image.texture_descriptor.mip_level_count = mip_levels;
    // Explicit, or a one-layer array would be viewed as a plain 2D texture.
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        label: Some("terrain_texture_array".to_string()),
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    });
    image
}

/// Bytes of one layer's RGBA8 mip chain at `resolution`, 1x1 included.
pub fn layer_chain_len(resolution: u32) -> usize {
    (0..mip_level_count(resolution, resolution))
        .map(|level| {
            let side = (resolution >> level).max(1) as usize;
            side * side * 4
        })
        .sum()
}

/// A whole layer chain of one texel.
fn flat_chain(resolution: u32, texel: [u8; 4]) -> Vec<u8> {
    texel.as_slice().repeat(layer_chain_len(resolution) / 4)
}

/// The mip chain at `resolution` of a square power-of-two map `size` texels
/// on a side, `size >= resolution`: the texture-gen chain of the whole map
/// with its levels above `resolution` cut off, so the downscale is the same
/// filter as every mip below it (linear light for colour, renormalised
/// vectors for normals). `None` for a map that is not such a square.
fn chain_from_pow2(level0: Vec<u8>, size: u32, resolution: u32, kind: MapKind) -> Option<Vec<u8>> {
    let pow2_square = size.is_power_of_two()
        && resolution.is_power_of_two()
        && size >= resolution
        && level0.len() == size as usize * size as usize * 4;
    if !pow2_square {
        return None;
    }
    let (mut chain, _) = build_mip_chain_rgba8(level0, size, size, kind);
    let skip = (size / resolution).trailing_zeros();
    let above: usize = (0..skip)
        .map(|level| {
            let side = (size >> level) as usize;
            side * side * 4
        })
        .sum();
    let tail = chain.split_off(above);
    (tail.len() == layer_chain_len(resolution)).then_some(tail)
}

/// Layer stats from its albedo and ORM chains: the last texel of a chain is
/// its 1x1 mip, the box-filtered mean.
fn layer_stats(set: TerrainTextureSet, loaded: bool, albedo: &[u8], orm: &[u8]) -> TerrainTextureLayer {
    let last = |chain: &[u8]| -> [f32; 4] {
        let texel = chain.len().checked_sub(4).map_or([0u8; 4], |at| [chain[at], chain[at + 1], chain[at + 2], chain[at + 3]]);
        texel.map(|c| c as f32 / 255.0)
    };
    let [r, g, b, _] = last(albedo);
    let [occlusion, roughness, metallic, _] = last(orm);
    TerrainTextureLayer {
        set,
        loaded,
        mean_albedo_linear: [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b)],
        mean_occlusion: occlusion,
        mean_roughness: roughness,
        mean_metallic: metallic,
    }
}

/// One finished layer.
struct LayerPixels {
    layer: TerrainTextureLayer,
    albedo: Vec<u8>,
    normal: Vec<u8>,
    orm: Vec<u8>,
}

impl LayerPixels {
    /// The white placeholder for a set whose albedo did not load.
    fn placeholder(set: TerrainTextureSet, resolution: u32) -> Self {
        let albedo = flat_chain(resolution, MISSING_ALBEDO_TEXEL);
        let orm = flat_chain(resolution, DEFAULT_ORM_TEXEL);
        Self {
            layer: layer_stats(set, false, &albedo, &orm),
            normal: flat_chain(resolution, FLAT_NORMAL_TEXEL),
            albedo,
            orm,
        }
    }
}

/// Packs finished layers, in array order, into three layer-major buffers.
/// Each layer is copied in and dropped as it arrives, so a build holds the
/// arrays plus the one layer being built, not every layer twice.
struct ArrayAssembler {
    resolution: u32,
    per_layer: usize,
    albedo: Vec<u8>,
    normal: Vec<u8>,
    orm: Vec<u8>,
    layers: Vec<TerrainTextureLayer>,
}

impl ArrayAssembler {
    fn new(resolution: u32, layer_count: usize) -> Self {
        let per_layer = layer_chain_len(resolution);
        let total = per_layer * layer_count;
        Self {
            resolution,
            per_layer,
            albedo: Vec::with_capacity(total),
            normal: Vec::with_capacity(total),
            orm: Vec::with_capacity(total),
            layers: Vec::with_capacity(layer_count),
        }
    }

    /// Append the next layer. A chain of the wrong length fails the build,
    /// since wgpu would reject the whole upload.
    fn push(&mut self, layer: LayerPixels) -> Result<(), String> {
        let per_layer = self.per_layer;
        if layer.albedo.len() != per_layer || layer.normal.len() != per_layer || layer.orm.len() != per_layer {
            return Err(format!(
                "texture set {} built {}/{}/{} bytes per map, expected {per_layer}",
                layer.layer.set.label(),
                layer.albedo.len(),
                layer.normal.len(),
                layer.orm.len()
            ));
        }
        self.albedo.extend_from_slice(&layer.albedo);
        self.normal.extend_from_slice(&layer.normal);
        self.orm.extend_from_slice(&layer.orm);
        self.layers.push(layer.layer);
        Ok(())
    }

    /// The packed arrays. Fails when there are no layers, or none loaded.
    fn finish(self) -> Result<BuiltArrays, String> {
        if self.layers.is_empty() {
            return Err("the slot table uses no texture sets".to_string());
        }
        if self.layers.iter().all(|layer| !layer.loaded) {
            return Err(format!("none of the {} terrain texture sets could be read", self.layers.len()));
        }
        Ok(BuiltArrays {
            resolution: self.resolution,
            mip_levels: mip_level_count(self.resolution, self.resolution),
            albedo: self.albedo,
            normal: self.normal,
            orm: self.orm,
            layers: self.layers,
        })
    }
}

/// [`ArrayAssembler`] over layers already in hand.
#[cfg(test)]
fn assemble_arrays(layers: Vec<LayerPixels>, resolution: u32) -> Result<BuiltArrays, String> {
    let mut assembler = ArrayAssembler::new(resolution, layers.len());
    for layer in layers {
        assembler.push(layer)?;
    }
    assembler.finish()
}

/// Build every layer of `key` on one async compute task, one set after
/// another, packing each as it finishes.
#[cfg(feature = "image")]
fn spawn_build(key: BuildKey, bundled_root: std::path::PathBuf) -> Task<Result<BuiltArrays, String>> {
    use bevy::tasks::AsyncComputeTaskPool;
    AsyncComputeTaskPool::get().spawn(async move {
        let resolution = key.resolution;
        let mut assembler = ArrayAssembler::new(resolution, key.sets.len());
        for set in &key.sets {
            assembler.push(build_layer(set.clone(), set.files(&bundled_root), resolution))?;
            // A decode never awaits, and Bevy compiles render pipelines on
            // this same pool (1 to 4 threads by default). Yielding between
            // sets lets the pipelines queued at Space open run instead of
            // waiting out the whole build; it is also where a superseded
            // build, whose task was dropped, stops.
            bevy::tasks::futures_lite::future::yield_now().await;
        }
        assembler.finish()
    })
}

/// Decode one set's maps and build their chains.
#[cfg(feature = "image")]
fn build_layer(
    set: TerrainTextureSet,
    files: super::material_slots::TextureSetFiles,
    resolution: u32,
) -> LayerPixels {
    let albedo = match load_layer_chain(&files.albedo, resolution, MapKind::Color) {
        Ok(chain) => chain,
        Err(error) => {
            warn!(target: "eustress::terrain::textures", "texture set {}: {error}", set.label());
            return LayerPixels::placeholder(set, resolution);
        }
    };
    let optional = |path: Option<&std::path::Path>, kind: MapKind, flat: [u8; 4]| -> Vec<u8> {
        let Some(path) = path else {
            return flat_chain(resolution, flat);
        };
        load_layer_chain(path, resolution, kind).unwrap_or_else(|error| {
            warn!(target: "eustress::terrain::textures", "texture set {}: {error}; using a flat map", set.label());
            flat_chain(resolution, flat)
        })
    };
    let normal = optional(files.normal.as_deref(), MapKind::Normal, FLAT_NORMAL_TEXEL);
    let orm = optional(files.orm.as_deref(), MapKind::Data, DEFAULT_ORM_TEXEL);
    LayerPixels { layer: layer_stats(set, true, &albedo, &orm), albedo, normal, orm }
}

/// Decode the map at `path` and build its layer chain at `resolution`.
#[cfg(feature = "image")]
fn load_layer_chain(path: &std::path::Path, resolution: u32, kind: MapKind) -> Result<Vec<u8>, String> {
    let rgba = image::open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .into_rgba8();
    let (width, height) = rgba.dimensions();
    if width == height && width.is_power_of_two() && width >= resolution {
        return chain_from_pow2(rgba.into_raw(), width, resolution, kind)
            .ok_or_else(|| format!("{}: could not build its mip chain", path.display()));
    }
    // Any other shape is resized straight to the layer size. The filter
    // works on the stored values, so a colour map is filtered in sRGB here,
    // a small darkening on high-contrast detail that only non-square or
    // non-power-of-two sources pay.
    let resized = image::imageops::resize(&rgba, resolution, resolution, image::imageops::FilterType::Triangle);
    Ok(build_mip_chain_rgba8(resized.into_raw(), resolution, resolution, kind).0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::material::TerrainMaterial;
    use crate::terrain::material_slots::builtin_slot;

    #[test]
    fn layer_chains_hold_every_mip_down_to_one_texel() {
        assert_eq!(mip_level_count(1024, 1024), 11);
        assert_eq!(mip_level_count(512, 512), 10);
        // 4 * (1024^2 + 512^2 + ... + 1) = 4 * (4^11 - 1) / 3
        assert_eq!(layer_chain_len(1024), 4 * ((1usize << 22) - 1) / 3);
        assert_eq!(layer_chain_len(512), 4 * ((1usize << 20) - 1) / 3);
        assert_eq!(layer_chain_len(4), 4 * (16 + 4 + 1));
        assert_eq!(flat_chain(8, FLAT_NORMAL_TEXEL).len(), layer_chain_len(8));
    }

    #[test]
    fn a_larger_power_of_two_map_is_cut_down_to_the_layer_chain() {
        let size = 64u32;
        let level0 = vec![200u8; (size * size * 4) as usize];
        let chain = chain_from_pow2(level0, size, 16, MapKind::Data).unwrap();
        assert_eq!(chain.len(), layer_chain_len(16));
        assert!(chain.iter().all(|&b| b == 200), "a constant map stays constant down every mip");

        // Same size: the chain is the map's own.
        let same = chain_from_pow2(vec![7u8; 16 * 16 * 4], 16, 16, MapKind::Data).unwrap();
        assert_eq!(same.len(), layer_chain_len(16));

        // Smaller, non-square-sized or short data: not this path.
        assert!(chain_from_pow2(vec![0u8; 8 * 8 * 4], 8, 16, MapKind::Data).is_none());
        assert!(chain_from_pow2(vec![0u8; 48 * 48 * 4], 48, 16, MapKind::Data).is_none());
        assert!(chain_from_pow2(vec![0u8; 10], 64, 16, MapKind::Data).is_none());
    }

    #[test]
    fn a_downscaled_colour_layer_averages_in_linear_light() {
        // A 2x2 black/white checker cut to a 1x1 layer is the linear mean,
        // sRGB 188, and the layer stats read it back as linear 0.5.
        let mut level0 = Vec::new();
        for v in [255u8, 0, 0, 255] {
            level0.extend_from_slice(&[v, v, v, 255]);
        }
        let chain = chain_from_pow2(level0, 2, 1, MapKind::Color).unwrap();
        assert_eq!(chain.len(), 4);
        assert!((chain[0] as i32 - 188).abs() <= 1, "{chain:?}");
        let stats = layer_stats(TerrainTextureSet::Bundled("marble"), true, &chain, &flat_chain(1, DEFAULT_ORM_TEXEL));
        assert!((stats.mean_albedo_linear[0] - 0.5).abs() < 0.01);
        assert_eq!(stats.mean_roughness, 1.0);
    }

    #[test]
    fn layers_pack_layer_major_and_a_build_needs_one_loaded_set() {
        let resolution = 4;
        let loaded = |set: &'static str, texel: u8| {
            let albedo = flat_chain(resolution, [texel, texel, texel, 255]);
            let orm = flat_chain(resolution, [255, 128, 0, 255]);
            LayerPixels {
                layer: layer_stats(TerrainTextureSet::Bundled(set), true, &albedo, &orm),
                normal: flat_chain(resolution, FLAT_NORMAL_TEXEL),
                albedo,
                orm,
            }
        };
        let layers = vec![
            loaded("grass", 10),
            LayerPixels::placeholder(TerrainTextureSet::Bundled("gold"), resolution),
            loaded("sand", 30),
        ];
        let built = assemble_arrays(layers, resolution).unwrap();
        let per_layer = layer_chain_len(resolution);
        assert_eq!(built.mip_levels, 3);
        assert_eq!(built.albedo.len(), per_layer * 3);
        assert_eq!(built.albedo[0], 10);
        assert_eq!(built.albedo[per_layer], 255, "the placeholder layer is white");
        assert_eq!(built.albedo[2 * per_layer], 30);
        assert_eq!(built.albedo[per_layer - 4], 10, "a layer's own mips come before the next layer");
        assert!(!built.layers[1].loaded);
        assert!((built.layers[0].mean_roughness - 128.0 / 255.0).abs() < 1e-6);

        let unloaded = vec![LayerPixels::placeholder(TerrainTextureSet::Bundled("gold"), resolution)];
        assert!(assemble_arrays(unloaded, resolution).is_err());
        assert!(assemble_arrays(Vec::new(), resolution).is_err());
        let mut short = loaded("grass", 1);
        short.orm.pop();
        assert!(assemble_arrays(vec![short], resolution).is_err());
    }

    #[test]
    fn slots_on_one_set_share_its_layer_and_unloaded_sets_draw_flat() {
        let slots = TerrainMaterialSlots::builtins();
        let layer = |set: TerrainTextureSet, loaded: bool| TerrainTextureLayer {
            set,
            loaded,
            mean_albedo_linear: [0.5; 3],
            mean_occlusion: 1.0,
            mean_roughness: 0.5,
            mean_metallic: 0.0,
        };
        let layers: Vec<TerrainTextureLayer> = slots
            .unique_texture_sets()
            .into_iter()
            .map(|set| {
                let loaded = set != TerrainTextureSet::Bundled("brick");
                layer(set, loaded)
            })
            .collect();
        let arrays = TerrainTextureArrays {
            status: TerrainTextureStatus::Ready,
            current: Some(TerrainTextureArraySet {
                albedo: Handle::default(),
                normal: Handle::default(),
                orm: Handle::default(),
                layers,
                resolution: 4,
                mip_levels: 3,
            }),
            generation: 1,
            rebuild_requested: false,
        };
        let slot_of = |material: TerrainMaterial| material.to_u8();
        let sand = arrays.slot_layer(&slots, slot_of(TerrainMaterial::Sand));
        assert_eq!(sand, Some(2));
        for material in [TerrainMaterial::Dirt, TerrainMaterial::Mud, TerrainMaterial::Ground, TerrainMaterial::Sandstone] {
            assert_eq!(arrays.slot_layer(&slots, slot_of(material)), sand, "{material:?}");
        }
        assert_eq!(arrays.slot_layer(&slots, slot_of(TerrainMaterial::Grass)), Some(0));
        assert_eq!(arrays.slot_layer(&slots, slot_of(TerrainMaterial::LeafyGrass)), Some(0));

        let rock = arrays.slot_surface(&slots, slot_of(TerrainMaterial::Rock));
        let shipped = builtin_slot(TerrainMaterial::Rock);
        assert_eq!(rock.layer, Some(1));
        assert_eq!(rock.albedo, shipped.tint);
        assert!((rock.roughness_scale - shipped.roughness / 0.5).abs() < 1e-6);

        // Brick's layer did not load: flat, in its swatch.
        let brick = arrays.slot_surface(&slots, slot_of(TerrainMaterial::Brick));
        assert_eq!(brick.layer, None);
        assert_eq!(brick.albedo, builtin_slot(TerrainMaterial::Brick).swatch_linear());
        assert_eq!(brick.roughness_scale, 1.0);
        // An undefined slot is flat too.
        assert_eq!(arrays.slot_surface(&slots, 200).layer, None);
        // No arrays at all: everything is flat.
        assert_eq!(TerrainTextureArrays::default().slot_layer(&slots, 0), None);
    }

    #[test]
    fn layer_resolutions_are_the_two_documented_sizes() {
        assert_eq!(TerrainTextureSettings::default().layer_resolution.pixels(), 1024);
        assert_eq!(TerrainTextureResolution::R512.pixels(), 512);
        assert!(TerrainTextureSettings::default().enabled);
    }
}
