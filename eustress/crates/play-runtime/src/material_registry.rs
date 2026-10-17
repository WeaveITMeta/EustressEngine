//! # Material Loader
//!
//! Parses `.mat.toml` files from MaterialService into StandardMaterial handles.
//! Materials are deduplicated by (preset, color, transparency, reflectance) so
//! entities sharing identical visual properties share one GPU material handle,
//! enabling Bevy to batch their draw calls.
//!
//! ## Resolution Order (for Parts)
//! 1. MaterialRegistry — exact name match against loaded `.mat.toml` files
//! 2. Deduplication cache — share handles for identical (preset, color, transparency)
//! 3. Material enum fallback — `Material::from_string()` for 18 built-in presets
//! 4. Default — `Material::Plastic`

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use eustress_common::classes::Material as PresetMaterial;
use bevy::asset::RenderAssetUsages;
use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler,
    ImageSamplerDescriptor, ImageType,
};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use texture_gen::mips::{build_mip_chain_rgba8, MapKind};
use std::sync::Arc;
use eustress_common::play_session::PlaySpaceSource;
use eustress_common::space_source::SpaceSource;

// ============================================================================
// MaterialDefinition — the parsed .mat.toml structure
// ============================================================================

/// Top-level `.mat.toml` file structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialDefinition {
    #[serde(default)]
    pub material: MaterialMetadata,
    #[serde(default)]
    pub pbr: PbrProperties,
    #[serde(default)]
    pub textures: TextureProperties,
}

/// `[material]` section — name, preset, description
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MaterialMetadata {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub description: String,
}

/// `[pbr]` section — PBR visual properties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PbrProperties {
    #[serde(default = "default_base_color")]
    pub base_color: [f32; 4],
    pub roughness: Option<f32>,
    pub metallic: Option<f32>,
    pub reflectance: Option<f32>,
    #[serde(default = "default_alpha_mode")]
    pub alpha_mode: String,
    #[serde(default)]
    pub alpha_cutoff: f32,
    #[serde(default)]
    pub double_sided: bool,
    #[serde(default)]
    pub unlit: bool,
    // Tolerant: a `Vec<f32>` deserializes from a 3-float RGB or 4-float RGBA
    // array. A fixed `[f32; 4]` rejected the 3-component emissive that imported
    // materials (e.g. Neon = [2,2,2]) write, failing the WHOLE material parse.
    pub emissive: Option<Vec<f32>>,
    pub ior: Option<f32>,
    pub specular_transmission: Option<f32>,
    pub diffuse_transmission: Option<f32>,
    pub thickness: Option<f32>,
}

fn default_base_color() -> [f32; 4] { [1.0, 1.0, 1.0, 1.0] }
fn default_alpha_mode() -> String { "opaque".to_string() }

impl Default for PbrProperties {
    fn default() -> Self {
        Self {
            base_color: default_base_color(),
            roughness: None,
            metallic: None,
            reflectance: None,
            alpha_mode: default_alpha_mode(),
            alpha_cutoff: 0.5,
            double_sided: false,
            unlit: false,
            emissive: None,
            ior: None,
            specular_transmission: None,
            diffuse_transmission: None,
            thickness: None,
        }
    }
}

/// `[textures]` section — optional PBR texture paths
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TextureProperties {
    #[serde(default)]
    pub base_color: String,
    #[serde(default)]
    pub normal: String,
    #[serde(default)]
    pub metallic_roughness: String,
    #[serde(default)]
    pub emissive: String,
    #[serde(default)]
    pub occlusion: String,
    #[serde(default)]
    pub depth: String,
}

// ============================================================================
// MaterialRegistry — central handle cache
// ============================================================================

/// Central cache mapping material names to Bevy material handles.
/// Populated on Space load from `MaterialService/*.mat.toml` files.
#[derive(Resource, Default)]
pub struct MaterialRegistry {
    /// Name → loaded Bevy material handle
    materials: HashMap<String, Handle<StandardMaterial>>,
    /// Name → parsed definition (for property panel editing)
    definitions: HashMap<String, MaterialDefinition>,
    /// Name → source .mat.toml path (for writeback and hot-reload)
    source_paths: HashMap<String, PathBuf>,
    /// Deduplication cache: share handles for identical visual properties.
    /// Entities with the same color+preset share one GPU material → batched draws.
    dedup_cache: HashMap<MaterialCacheKey, Handle<StandardMaterial>>,
    /// Texture maps NOT yet loaded, keyed by material name; attached lazily on
    /// first use by `hydrate_textures` (a Space ships ~22 library materials with
    /// 2048² PBR sets — the measured scene used ONE; eager loading was ~1 GB of
    /// VRAM and, with Bevy's default CPU mirror, ~1 GB of RAM for nothing).
    pending_textures: HashMap<String, PendingTextures>,
    /// Materials whose maps have been attached.
    hydrated: std::collections::HashSet<String>,
    /// One GPU image per map file (by its asset URL) and filtering kind,
    /// shared by every material that references it (the library points
    /// `occlusion` and `metallic_roughness` at the same ORM file).
    texture_cache: HashMap<(String, MapKind), Handle<Image>>,
    /// The open Space's files when its shell reads them from somewhere other
    /// than a folder on this computer (the browser Player's records), set by
    /// [`register_space_materials_from`]; `None` for a folder. Where a map is
    /// looked for.
    space_source: Option<Arc<dyn SpaceSource>>,
}

/// Adaptive color-quantization shift for the dedup key. `0` == lossless
/// (8-bit exact per channel) — the default, so authored scenes are
/// byte-for-byte unchanged and there is zero visual regression. A
/// dense scene that overflows the load frame-budget flips this to a
/// few bits via [`set_dense_material_mode`], snapping near-identical
/// colors to a shared palette so 50k uniformly-random-colored parts
/// collapse from ~50k unique GPU materials (≈50k draw calls → ~4 FPS)
/// to ≤ a few thousand batched materials. Exact-keyed dedup already
/// handles real content (repeated colors); this only engages for the
/// pathological dense/random case (the benchmark) and never touches a
/// normal scene because a normal scene never spills.
static MATERIAL_QUANT_SHIFT: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

/// Bits dropped from each 8-bit color channel in dense mode. 4 → 16
/// levels/channel → ≤16³ = 4096 distinct colors (≈12× fewer draw
/// batches at 50k). Tunable in [3,5]: higher = fewer draws but more
/// visible banding (irrelevant for the random benchmark).
const DENSE_QUANT_SHIFT: u32 = 4;

/// Engage / disengage adaptive color quantization for dense scenes.
/// Called by the loader the first time a subtree overflows the
/// per-frame spawn budget (the precise "this scene is huge" signal),
/// and reset to lossless at the start of every fresh load.
pub fn set_dense_material_mode(on: bool) {
    MATERIAL_QUANT_SHIFT.store(
        if on { DENSE_QUANT_SHIFT } else { 0 },
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// True when adaptive quantization is currently engaged (diagnostics).
pub fn dense_material_mode() -> bool {
    MATERIAL_QUANT_SHIFT.load(std::sync::atomic::Ordering::Relaxed) != 0
}

/// Cache key for material deduplication. Quantizes floating-point material
/// parameters into integer bits so identical-looking materials hash together.
/// Two parts with the same color, preset, transparency, and reflectance
/// will share a single GPU material handle → single draw call batch.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MaterialCacheKey {
    /// RGBA color quantized to 8-bit per channel (4 bytes packed into u32)
    color_bits: u32,
    /// Material preset name (e.g. "Plastic", "Glass", "Neon") OR the custom
    /// `.mat.toml` registry name — the BASE-material identity. Two parts only
    /// share a handle when they tint the SAME base, so this distinguishes
    /// e.g. "Plastic" from "BrushedMetal".
    preset: String,
    /// Transparency quantized to 0-1000 (0.1% precision)
    transparency_millipct: u32,
    /// Reflectance quantized to 0-1000 (0.1% precision)
    reflectance_millipct: u32,
    /// UV tiling, quantized — `Some((u_milli, v_milli))` ONLY for TEXTURED
    /// base materials (where `uv_transform` is visible), `None` for untextured
    /// solid-color parts (the bulk + 100% of the benchmark). Gating on texture
    /// is what lets thousands of differently-SIZED untextured parts still share
    /// one material: their uv_transform varies but is invisible, so it must NOT
    /// fragment the key. A textured part of a different size correctly does not
    /// share (its tiling differs visibly).
    uv_bits: Option<(u32, u32)>,
}

impl MaterialCacheKey {
    /// `uv` is `Some(uv_transform)` only when the base material is TEXTURED
    /// (so tiling is visible); pass `None` for untextured parts.
    fn new(
        color: Color,
        preset_name: &str,
        transparency: f32,
        reflectance: f32,
        uv: Option<bevy::math::Affine2>,
    ) -> Self {
        let lin = LinearRgba::from(color);
        let r = (lin.red.clamp(0.0, 1.0) * 255.0) as u32;
        let g = (lin.green.clamp(0.0, 1.0) * 255.0) as u32;
        let b = (lin.blue.clamp(0.0, 1.0) * 255.0) as u32;
        let a = (lin.alpha.clamp(0.0, 1.0) * 255.0) as u32;
        // Adaptive quantization: shift == 0 (default) is a no-op
        // (`(c >> 0) << 0 == c`) so authored scenes are unchanged; a
        // dense scene snaps RGB to a coarse palette so near-identical
        // colors share one batched GPU material. Alpha is left exact
        // (transparency is keyed separately and affects sort order).
        let shift = MATERIAL_QUANT_SHIFT.load(std::sync::atomic::Ordering::Relaxed);
        let q = |c: u32| (c >> shift) << shift;
        let (r, g, b) = (q(r), q(g), q(b));
        // Quantize the uv scale (the diagonal of the Affine2's linear part) to
        // 0.1% so near-identical tilings share. Only kept for textured bases.
        let uv_bits = uv.map(|t| {
            let sx = (t.matrix2.x_axis.x.abs() * 1000.0) as u32;
            let sy = (t.matrix2.y_axis.y.abs() * 1000.0) as u32;
            (sx, sy)
        });
        Self {
            color_bits: (r << 24) | (g << 16) | (b << 8) | a,
            preset: preset_name.to_string(),
            transparency_millipct: (transparency.clamp(0.0, 1.0) * 1000.0) as u32,
            reflectance_millipct: (reflectance.clamp(0.0, 1.0) * 1000.0) as u32,
            uv_bits,
        }
    }
}

impl MaterialRegistry {
    /// Look up a material by name. Returns None if not in the registry.
    pub fn get(&self, name: &str) -> Option<Handle<StandardMaterial>> {
        self.materials.get(name).cloned()
    }

    /// Insert or update a material in the registry.
    pub fn insert(
        &mut self,
        name: String,
        handle: Handle<StandardMaterial>,
        definition: MaterialDefinition,
        source_path: PathBuf,
    ) {
        self.materials.insert(name.clone(), handle);
        self.definitions.insert(name.clone(), definition);
        self.source_paths.insert(name, source_path);
    }

    /// Remove a material by name.
    pub fn remove(&mut self, name: &str) {
        self.materials.remove(name);
        self.definitions.remove(name);
        self.source_paths.remove(name);
    }

    /// Get the parsed definition for a material (for property panel).
    pub fn get_definition(&self, name: &str) -> Option<&MaterialDefinition> {
        self.definitions.get(name)
    }

    /// List all registered material names.
    pub fn names(&self) -> Vec<String> {
        self.materials.keys().cloned().collect()
    }

    /// Number of loaded materials.
    pub fn len(&self) -> usize {
        self.materials.len()
    }

    /// Number of deduplicated material handles (shared across entities).
    pub fn dedup_cache_len(&self) -> usize {
        self.dedup_cache.len()
    }

    /// Look up or insert a deduplicated material handle by cache key.
    /// Crate-private: `MaterialCacheKey` is intentionally module-private,
    /// so the callers live inside `space::material_loader` too.
    pub(crate) fn dedup_get_or_insert(
        &mut self,
        key: MaterialCacheKey,
        handle: Handle<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        self.dedup_cache.entry(key).or_insert(handle).clone()
    }

    /// R2.1 — resolve a Part's tinted material to a SHARED, deduplicated
    /// `Handle<StandardMaterial>` with copy-on-write semantics. This is the
    /// scale lever: `material_sync` calls this instead of `materials.add(clone)`
    /// per entity, so identical-appearance parts share ONE handle → Bevy batches
    /// them into instanced/indirect draws (a unique handle per entity is what
    /// forces one draw call per entity — bevy_pbr batch key = material bind
    /// group + mesh).
    ///
    /// Two build paths:
    /// - `base_template = Some(h)`: clone the registry's (possibly textured)
    ///   base material `h` and tint it with the part's colour.
    /// - `base_template = None`: build from the `preset` PBR params (formerly
    ///   an in-place `get_mut`, which is the "edit one, change all" trap once
    ///   handles are shared, so it is a fresh build + dedup instead).
    ///
    /// In both, the part's `Reflectance` only ever RAISES the material's own
    /// specular (`reflectance.max(..)`). It is the Eustress mirror knob and
    /// defaults to 0, while `StandardMaterial::reflectance = 0` means an F0 of
    /// zero (no highlight, no environment reflection), so it must never
    /// replace the material's own value.
    ///
    /// Copy-on-write: the returned handle is keyed on the FULL appearance, so
    /// editing one part's `BasePart` re-resolves it to the handle matching its
    /// NEW appearance (shared with others of that look, or freshly inserted)
    /// WITHOUT mutating any other part's material.
    ///
    /// `uv_transform` is applied + keyed ONLY when the base is textured;
    /// untextured parts get `Affine2::IDENTITY` (invisible) so differently-sized
    /// solid-color parts still share.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resolve_part_material(
        &mut self,
        materials: &mut Assets<StandardMaterial>,
        asset_server: &AssetServer,
        mat_name: &str,
        preset: PresetMaterial,
        base_template: Option<Handle<StandardMaterial>>,
        color: Color,
        transparency: f32,
        reflectance: f32,
        uv_transform: bevy::math::Affine2,
    ) -> Handle<StandardMaterial> {
        // Attach the library material's texture maps NOW, before anything
        // below looks at the template. Library materials register WITHOUT
        // maps (so the ~20 unused ones cost nothing); this first use is the
        // moment they are needed. It has to happen here rather than in a
        // later system because the code below (a) decides `textured` from the
        // template's `base_color_texture` and (b) SNAPSHOTS the template into
        // an owned per-key material — a hydration after that point would never
        // reach the parts already built from the snapshot.
        if let Some(h) = base_template.as_ref() {
            if let Some(key) = self.name_for_handle(h).map(|s| s.to_string()) {
                self.hydrate_textures(&key, asset_server, materials);
            }
        }
        let alpha = 1.0 - transparency.clamp(0.0, 1.0);
        let reflectance = reflectance.clamp(0.0, 1.0);
        let is_glass = matches!(preset, PresetMaterial::Glass);
        let is_neon = matches!(preset, PresetMaterial::Neon);

        // Is the base material textured? Only then does uv_transform matter
        // (and only then do we key on it, so untextured parts of varying size
        // still share). Resolved from the base template's base_color_texture.
        let textured = base_template
            .as_ref()
            .and_then(|h| materials.get(h))
            .map(|m| m.base_color_texture.is_some())
            .unwrap_or(false);
        let effective_uv = if textured {
            uv_transform
        } else {
            bevy::math::Affine2::IDENTITY
        };

        let cache_key = MaterialCacheKey::new(
            color,
            mat_name,
            transparency,
            reflectance,
            if textured { Some(uv_transform) } else { None },
        );
        if let Some(handle) = self.dedup_cache.get(&cache_key) {
            return handle.clone();
        }

        // Build once on miss. Snapshot the base material into an OWNED value
        // first so the immutable borrow of `materials` is released before the
        // `materials.add(mat)` below (no borrow held across the mutate). Cheap:
        // only reached on a cache miss, not on the common hit above.
        let base_clone: Option<StandardMaterial> =
            base_template.as_ref().and_then(|h| materials.get(h)).cloned();
        let mat = match base_clone {
            // Registry-clone branch (old branches 1+2 math): clone the base
            // (keeps its textures + baked roughness) then tint.
            Some(mut cloned) => {
                cloned.base_color = color.with_alpha(alpha);
                if transparency > 0.0 {
                    cloned.alpha_mode = AlphaMode::Blend;
                }
                cloned.reflectance = cloned.reflectance.max(reflectance);
                cloned.metallic = (cloned.metallic + reflectance).min(1.0);
                cloned.perceptual_roughness *= 1.0 - reflectance * 0.5;
                if is_neon {
                    cloned.emissive = LinearRgba::from(color) * 2.0;
                }
                cloned.uv_transform = effective_uv;
                cloned
            }
            // Preset-scratch branch (old in-place `get_mut` math, now built
            // fresh so the handle can be SHARED instead of mutated in place).
            None => {
                let (preset_roughness, _preset_metallic, preset_reflectance) = preset.pbr_params();
                let mut mat = StandardMaterial {
                    base_color: color.with_alpha(alpha),
                    alpha_mode: if transparency > 0.0 {
                        AlphaMode::Blend
                    } else {
                        AlphaMode::Opaque
                    },
                    metallic: reflectance,
                    perceptual_roughness: preset_roughness * (1.0 - reflectance * 0.5),
                    reflectance: preset_reflectance.max(reflectance),
                    uv_transform: effective_uv,
                    ..default()
                };
                if is_glass {
                    mat.specular_transmission = 0.9;
                    mat.diffuse_transmission = 0.3;
                    mat.thickness = 0.5;
                    mat.ior = 1.5;
                }
                if is_neon {
                    mat.emissive = LinearRgba::from(color) * 2.0;
                }
                mat
            }
        };

        let handle = materials.add(mat);
        self.dedup_get_or_insert(cache_key, handle)
    }
}

// ============================================================================
// Parsing and Loading
// ============================================================================

/// Parse a `.mat.toml` file into a MaterialDefinition.
/// A Space's own materials into the registry, as Studio's loader registers
/// them when it opens a Space: every `MaterialService/**/*.mat.toml`, by the
/// name inside it or else its file name. Maps attach the first time a part
/// uses the material. Returns how many registered, and the files and
/// folders that did not read.
pub fn register_space_materials(
    space_root: &Path,
    registry: &mut MaterialRegistry,
    materials: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
) -> (usize, Vec<String>) {
    registry.space_source = None;
    let mut registered = 0;
    let (files, mut failed) = disk_material_files(space_root);
    for path in files {
        match register_material(load_material_definition(&path), &path, space_root, registry, materials, asset_server) {
            Ok(()) => registered += 1,
            Err(e) => failed.push(e),
        }
    }
    (registered, failed)
}

/// [`register_space_materials`] for a Space its shell reads from `source`
/// rather than a folder on this computer (the browser Player's records):
/// its material files are listed and read through the source, and their
/// maps are looked for in it ([`texture_source_in`]). `space_root` is the
/// `PlayAssetRoot` the shell gave the Space.
pub fn register_space_materials_from(
    source: &PlaySpaceSource,
    space_root: &Path,
    registry: &mut MaterialRegistry,
    materials: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
) -> (usize, Vec<String>) {
    registry.space_source = Some(source.0.clone());
    let mut registered = 0;
    let (files, mut failed) = source_material_files(&*source.0);
    for rel in files {
        let path = space_root.join(&rel);
        let definition = source
            .0
            .read_to_string(&rel)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))
            .and_then(|content| load_material_definition_from_str(&content));
        match register_material(definition, &path, space_root, registry, materials, asset_server) {
            Ok(()) => registered += 1,
            Err(e) => failed.push(e),
        }
    }
    (registered, failed)
}

/// Every `MaterialService/**/*.mat.toml` in the Space's folder, in path
/// order, and each folder that could not be listed. Hidden files and folders
/// are skipped; a linked folder is followed. A Space without MaterialService
/// has none and reports nothing.
fn disk_material_files(space_root: &Path) -> (Vec<PathBuf>, Vec<String>) {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>, problems: &mut Vec<String>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                problems.push(format!("Failed to list {}: {}", dir.display(), e));
                return;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                // A failed entry ends the listing (on Windows for good), so
                // what the folder holds past it is unknown.
                Err(e) => {
                    problems.push(format!("Failed to list all of {}: {}", dir.display(), e));
                    break;
                }
            };
            let path = entry.path();
            let hidden = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.'));
            if hidden {
                continue;
            }
            if path.is_dir() {
                walk(&path, out, problems);
            } else if path.to_string_lossy().ends_with(".mat.toml") {
                out.push(path);
            }
        }
    }
    let (mut files, mut problems) = (Vec::new(), Vec::new());
    walk(&space_root.join("MaterialService"), &mut files, &mut problems);
    files.sort();
    (files, problems)
}

/// [`disk_material_files`] from a [`SpaceSource`]: Space-relative paths, in
/// the same order, and the folders that could not be listed.
fn source_material_files(source: &dyn SpaceSource) -> (Vec<String>, Vec<String>) {
    fn walk(source: &dyn SpaceSource, dir: &str, out: &mut Vec<String>, problems: &mut Vec<String>) {
        let entries = match source.list(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                problems.push(format!("Failed to list {dir}: {e}"));
                return;
            }
        };
        for entry in entries {
            if entry.name.starts_with('.') {
                continue;
            }
            if entry.is_dir {
                walk(source, &entry.rel_path, out, problems);
            } else if entry.name.ends_with(".mat.toml") {
                out.push(entry.rel_path);
            }
        }
    }
    let (mut files, mut problems) = (Vec::new(), Vec::new());
    walk(source, "MaterialService", &mut files, &mut problems);
    files.sort_by(|a, b| Path::new(a).cmp(Path::new(b)));
    (files, problems)
}

/// One material into the registry, by the name inside it or else its file
/// name, its maps left to attach when a part first uses it.
fn register_material(
    definition: Result<MaterialDefinition, String>,
    path: &Path,
    space_root: &Path,
    registry: &mut MaterialRegistry,
    materials: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
) -> Result<(), String> {
    let definition = definition?;
    let name = if definition.material.name.is_empty() {
        material_name_from_path(path)
    } else {
        definition.material.name.clone()
    };
    let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let handle = materials.add(build_standard_material(&definition, asset_server, &dir, space_root));
    registry.insert(name.clone(), handle, definition.clone(), path.to_path_buf());
    registry.defer_textures(&name, &definition.textures, &dir, space_root);
    Ok(())
}

pub fn load_material_definition(path: &Path) -> Result<MaterialDefinition, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    load_material_definition_from_str(&content)
}

/// In-memory twin — parse a `.mat.toml` from content the caller
/// already sourced through `SpaceSource`. No `std::fs`.
///
/// A Space's copy of a library material that is an unedited earlier release
/// is read as the current release (see [`SUPERSEDED_LIBRARY_VERSIONS`]).
pub fn load_material_definition_from_str(content: &str) -> Result<MaterialDefinition, String> {
    let content = match current_library_version(content) {
        Some((name, current)) => {
            debug!("🎨 material '{}': unedited earlier library version, reading the current one", name);
            current
        }
        None => content,
    };
    toml::from_str::<MaterialDefinition>(content)
        .map_err(|e| format!("Failed to parse material TOML: {}", e))
}

// ============================================================================
// Library materials
// ============================================================================

macro_rules! library_file {
    ($name:literal) => {
        (
            $name,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../common/assets/service_templates/MaterialService/",
                $name,
                ".mat.toml"
            )),
        )
    };
}

/// The textured library materials as the engine ships them. Every Space keeps
/// its own copy of each file in `MaterialService/`, seeded from these.
const LIBRARY: &[(&str, &str)] = &[
    library_file!("Brick"),
    library_file!("Bronze"),
    library_file!("Concrete"),
    library_file!("CorrodedMetal"),
    library_file!("DiamondPlate"),
    library_file!("Fabric"),
    library_file!("Foil"),
    library_file!("Gold"),
    library_file!("Granite"),
    library_file!("Grass"),
    library_file!("Ice"),
    library_file!("Marble"),
    library_file!("Metal"),
    library_file!("Sand"),
    library_file!("Silver"),
    library_file!("Slate"),
    library_file!("Wood"),
    library_file!("WoodPlanks"),
];

/// Digests of every earlier shipped version of a [`LIBRARY`] file. A Space
/// copy that matches one was never edited, so it is read as the current
/// version: the library improves in every existing Space, while a copy anyone
/// has changed, even by one value, is used exactly as written. Nothing on disk
/// is touched.
///
/// Digest: SHA-256 of the file with every space, tab, CR and LF removed, first
/// 16 hex digits (`tr -d ' \t\r\n' < file | sha256sum | cut -c1-16`).
const SUPERSEDED_LIBRARY_VERSIONS: &[(&str, &str)] = &[
    ("8192f189b2efcfd0", "Brick"),
    ("0d03ab8dc11033a9", "Bronze"),
    ("c4179e9403b14c92", "Bronze"),
    ("b6525465f4da65d2", "Concrete"),
    ("b8360ad353304185", "CorrodedMetal"),
    ("d58eff07e4f25e55", "DiamondPlate"),
    ("99bb4cbfceb00f05", "Fabric"),
    ("99c0560696632afd", "Foil"),
    ("0f23318fe7473225", "Gold"),
    ("b5488ce716be7c6d", "Gold"),
    ("d0e27edc49836c5e", "Granite"),
    ("bbb1d63d8b342303", "Grass"),
    ("deaed6963d78adab", "Ice"),
    ("6f3a8237fc387637", "Marble"),
    ("0a12c50db0281462", "Metal"),
    ("0c3194a2dd92d443", "Sand"),
    ("199106bd745bf23b", "Silver"),
    ("fcfc904fe9a2f446", "Silver"),
    ("7d5bcdd62d4d149a", "Slate"),
    ("959c40d1b6e33ce3", "Wood"),
    ("af14376f309c20dc", "WoodPlanks"),
];

fn library_digest(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let stripped: Vec<u8> = content
        .bytes()
        .filter(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        .collect();
    Sha256::digest(&stripped)
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `(name, current text)` when `content` is an unedited earlier library version.
fn current_library_version(content: &str) -> Option<(&'static str, &'static str)> {
    let digest = library_digest(content);
    let (_, name) = SUPERSEDED_LIBRARY_VERSIONS.iter().find(|(d, _)| *d == digest)?;
    LIBRARY.iter().find(|(n, _)| n == name).copied()
}

/// Extract a material name from its file path (stem before first dot).
pub fn material_name_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unnamed")
        .split('.')
        .next()
        .unwrap_or("unnamed")
        .to_string()
}

/// Build a Bevy `StandardMaterial` from a `MaterialDefinition`.
///
/// If `preset` is set, missing PBR fields inherit from the Material enum's `pbr_params()`.
/// Textures are loaded via the AssetServer using the `space://` source.
pub fn build_standard_material(
    definition: &MaterialDefinition,
    asset_server: &AssetServer,
    mat_toml_dir: &Path,
    space_root: &Path,
) -> StandardMaterial {
    // Resolve preset defaults (roughness, metallic, reflectance)
    let (preset_roughness, preset_metallic, preset_reflectance) =
        if let Some(ref preset_name) = definition.material.preset {
            PresetMaterial::from_string(preset_name).pbr_params()
        } else {
            // No preset — use sensible defaults
            (0.5, 0.0, 0.5)
        };

    let pbr = &definition.pbr;
    let [r, g, b, a] = pbr.base_color;

    let alpha_mode = match pbr.alpha_mode.to_lowercase().as_str() {
        "blend" => AlphaMode::Blend,
        "mask" => AlphaMode::Mask(pbr.alpha_cutoff),
        _ => {
            if a < 1.0 {
                AlphaMode::Blend
            } else {
                AlphaMode::Opaque
            }
        }
    };

    let mut mat = StandardMaterial {
        base_color: Color::srgba(r, g, b, a),
        metallic: pbr.metallic.unwrap_or(preset_metallic),
        perceptual_roughness: pbr.roughness.unwrap_or(preset_roughness),
        reflectance: pbr.reflectance.unwrap_or(preset_reflectance),
        alpha_mode,
        double_sided: pbr.double_sided,
        unlit: pbr.unlit,
        ..default()
    };

    // Optional PBR fields
    if let Some(e) = pbr.emissive.as_ref() {
        // Accept 3-float RGB (pad alpha = 1.0) or 4-float RGBA.
        let er = e.first().copied().unwrap_or(0.0);
        let eg = e.get(1).copied().unwrap_or(0.0);
        let eb = e.get(2).copied().unwrap_or(0.0);
        let ea = e.get(3).copied().unwrap_or(1.0);
        mat.emissive = LinearRgba::new(er, eg, eb, ea) * 1.0;
    }
    if let Some(ior) = pbr.ior {
        mat.ior = ior;
    }
    if let Some(st) = pbr.specular_transmission {
        mat.specular_transmission = st;
    }
    if let Some(dt) = pbr.diffuse_transmission {
        mat.diffuse_transmission = dt;
    }
    if let Some(th) = pbr.thickness {
        mat.thickness = th;
    }

    // Texture maps are NOT loaded here. They are attached lazily by
    // `MaterialRegistry::hydrate_textures` the first time a Part uses this
    // material (see `material_sync::hydrate_used_material_textures`), so a
    // library material nobody has applied costs no VRAM and no RAM.
    mat
}

/// [`texture_source_in`] for a Space read from its folder.
#[cfg(test)]
fn texture_source(
    mat_toml_dir: &Path,
    relative_path: &str,
    space_root: &Path,
    bundled_root: &Path,
) -> Result<(PathBuf, String), String> {
    texture_source_in(None, mat_toml_dir, relative_path, space_root, bundled_root)
}

/// The file a material's texture path names, and its asset path: 1. relative
/// to the .mat.toml directory, inside the Space (`space://...`), else 2. in the
/// bundled library (`bundled://materials/textures/brick_base_color.png`). A
/// path is the Space's own: one that leaves its folder (`..`, a drive or share,
/// a `:`) or reaches out through a link is refused (`space_paths`). The `Err`
/// is the warning to log. The Space's files are read from `space` when its
/// shell gave one (the browser Player's records), else from its folder. A
/// browser cannot look in the bundled library, so there a map the Space does
/// not hold is named in the library unseen, and its reader answers when the
/// map is read.
fn texture_source_in(
    space: Option<&dyn SpaceSource>,
    mat_toml_dir: &Path,
    relative_path: &str,
    space_root: &Path,
    bundled_root: &Path,
) -> Result<(PathBuf, String), String> {
    let Some(rel) = eustress_common::animation::content::safe_relative(relative_path) else {
        return Err(format!("Texture path {relative_path:?} leaves its folder; not loaded"));
    };
    let in_space = mat_toml_dir.join(&rel);
    if let Ok(inside) = in_space.strip_prefix(space_root) {
        let inside = inside.to_string_lossy().replace('\\', "/");
        let found = match space {
            Some(space) => space.exists(&inside),
            None => in_space.exists() && crate::space_paths::is_within(&in_space, space_root),
        };
        if found {
            return Ok((in_space, format!("space://{inside}")));
        }
    }
    let bundled = bundled_root.join(&rel);
    let in_library = cfg!(target_arch = "wasm32")
        || (bundled.exists() && crate::space_paths::is_within(&bundled, bundled_root));
    if in_library {
        return Ok((bundled, format!("bundled://{rel}")));
    }
    Err(format!("Texture not found: {in_space:?} (not in space or bundled assets)"))
}

/// Load a material map relative to the .mat.toml directory (the Space), falling
/// back to the bundled library (`common/assets/`). One image per map URL and
/// filtering kind, shared through `cache`.
fn load_texture(
    asset_server: &AssetServer,
    cache: &mut HashMap<(String, MapKind), Handle<Image>>,
    space: Option<&dyn SpaceSource>,
    mat_toml_dir: &Path,
    relative_path: &str,
    space_root: &Path,
    kind: MapKind,
) -> Option<Handle<Image>> {
    let url = match texture_source_in(space, mat_toml_dir, relative_path, space_root, &eustress_common::assets_dir()) {
        Ok((_, url)) => url,
        Err(why) => {
            warn!("{why}");
            return None;
        }
    };
    let handle = cache
        .entry((url.clone(), kind))
        .or_insert_with(|| load_image(asset_server, url, kind))
        .clone();
    Some(handle)
}

// ============================================================================
// Resolve material for a Part — registry-first, enum fallback
// ============================================================================

/// Resolve a material name to a `Handle<StandardMaterial>`.
///
/// 1. Check `MaterialRegistry` for a loaded `.mat.toml` handle
/// 2. Check deduplication cache — share handles for identical visual properties
/// 3. Fall back to creating a new material from the `Material` enum preset
pub fn resolve_material(
    material_name: &str,
    registry: &mut MaterialRegistry,
    materials: &mut Assets<StandardMaterial>,
    base_color: Color,
    transparency: f32,
    reflectance: f32,
) -> Handle<StandardMaterial> {
    // 1. Registry lookup (custom .mat.toml materials)
    if let Some(handle) = registry.get(material_name) {
        return handle;
    }

    // 2. Dedup cache lookup — share handles for identical (preset, color, transparency).
    // Spawn-time resolve is untextured (no BasePart size here) → uv = None.
    let cache_key = MaterialCacheKey::new(base_color, material_name, transparency, reflectance, None);
    if let Some(handle) = registry.dedup_cache.get(&cache_key) {
        return handle.clone();
    }

    // 3. Build a new material from preset
    let preset = PresetMaterial::from_string(material_name);
    let (roughness, metallic, preset_reflectance) = preset.pbr_params();

    let alpha = 1.0 - transparency;
    let mut mat = StandardMaterial {
        base_color: base_color.with_alpha(alpha),
        alpha_mode: if alpha < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
        perceptual_roughness: roughness,
        metallic,
        reflectance: if reflectance > 0.0 { reflectance } else { preset_reflectance },
        ..default()
    };

    // Special handling for Glass
    if matches!(preset, PresetMaterial::Glass) {
        mat.specular_transmission = 0.9;
        mat.diffuse_transmission = 0.3;
        mat.thickness = 0.5;
        mat.ior = 1.5;
    }

    // Special handling for Neon — emissive glow
    if matches!(preset, PresetMaterial::Neon) {
        let lin = LinearRgba::from(base_color);
        mat.emissive = lin * 2.0;
    }

    let handle = materials.add(mat);
    // Cache for future entities with identical properties
    registry.dedup_get_or_insert(cache_key, handle)
}

// ============================================================================
// ECS Component for material entities in Explorer
// ============================================================================

/// Marker component for material definition entities spawned from `.mat.toml`.
/// Allows the Explorer and Properties panel to identify and display materials.
#[derive(Component, Debug, Clone)]
pub struct MaterialDefinitionComponent {
    /// Material name (filename stem)
    pub name: String,
    /// Source .mat.toml path
    pub source_path: PathBuf,
}

/// Spawn an ECS entity representing a material definition for the Explorer tree.
/// This entity is non-visual — it exists only so the Explorer can list materials
/// and the Properties panel can show/edit their PBR fields.
pub fn spawn_material_entity(
    commands: &mut Commands,
    path: PathBuf,
    definition: &MaterialDefinition,
) -> Entity {
    let name = if definition.material.name.is_empty() {
        material_name_from_path(&path)
    } else {
        definition.material.name.clone()
    };

    commands.spawn((
        Name::new(name.clone()),
        eustress_common::classes::Instance {
            name: name.clone(),
            class_name: eustress_common::classes::ClassName::Material,
            archivable: true,
            id: 0,
            ..Default::default()
        },
        MaterialDefinitionComponent {
            name,
            source_path: path,
        },
    )).id()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A texture path loads from the Space or the bundled library, and never
    /// from anywhere else: `..` and an absolute path are refused even when
    /// the file they name exists.
    #[test]
    fn a_texture_path_stays_inside_the_space() {
        let base = std::env::temp_dir().join(format!("eustress_texture_source_{}", std::process::id()));
        let space = base.join("space");
        let mats = space.join("MaterialService");
        let bundled = base.join("bundled");
        std::fs::create_dir_all(mats.join("textures")).unwrap();
        std::fs::create_dir_all(bundled.join("materials")).unwrap();
        std::fs::write(mats.join("textures/brick.png"), b"x").unwrap();
        std::fs::write(bundled.join("materials/stone.png"), b"x").unwrap();
        std::fs::write(base.join("secret.png"), b"x").unwrap();

        let (disk, url) = texture_source(&mats, "textures/brick.png", &space, &bundled).expect("in the Space");
        assert_eq!((disk, url.as_str()), (mats.join("textures/brick.png"), "space://MaterialService/textures/brick.png"));
        let (_, url) = texture_source(&mats, "materials/stone.png", &space, &bundled).expect("bundled");
        assert_eq!(url, "bundled://materials/stone.png");

        let absolute = base.join("secret.png").to_string_lossy().into_owned();
        for bad in ["../../secret.png", "textures/../../../secret.png", absolute.as_str(), r"\\host\share\x.png", "textures/brick.png:zone"] {
            assert!(texture_source(&mats, bad, &space, &bundled).is_err(), "{bad:?}");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A texture path resolves alike from the Space's folder and from its
    /// records (the browser Player's): the same URL for a map both hold, the
    /// bundled library's for one neither holds, the same refusals. What the
    /// records hold, and only that, decides for the records.
    #[test]
    fn a_texture_resolves_alike_from_the_folder_and_its_records() {
        let base = std::env::temp_dir().join(format!("eustress_texture_records_{}", std::process::id()));
        let space = base.join("space");
        let mats = space.join("MaterialService");
        let bundled = base.join("bundled");
        std::fs::create_dir_all(mats.join("textures")).unwrap();
        std::fs::create_dir_all(bundled.join("materials")).unwrap();
        std::fs::write(mats.join("textures/brick.png"), b"x").unwrap();
        std::fs::write(mats.join("textures/folder_only.png"), b"x").unwrap();
        std::fs::write(bundled.join("materials/stone.png"), b"x").unwrap();
        let records = eustress_common::space_source::RecordsSource::new(vec![
            ("MaterialService/Brick.mat.toml".to_string(), b"[material]\n".to_vec()),
            ("MaterialService/textures/brick.png".to_string(), b"x".to_vec()),
            ("MaterialService/textures/records_only.png".to_string(), b"x".to_vec()),
        ]);
        let url = |space_source: Option<&dyn SpaceSource>, rel: &str| {
            texture_source_in(space_source, &mats, rel, &space, &bundled).map(|(_, url)| url)
        };
        for rel in ["textures/brick.png", "./textures//brick.png", "textures\\brick.png", "materials/stone.png", "textures/none.png"] {
            assert_eq!(url(None, rel), url(Some(&records), rel), "{rel:?}");
        }
        assert_eq!(url(Some(&records), "textures/brick.png").unwrap(), "space://MaterialService/textures/brick.png");
        assert_eq!(url(Some(&records), "materials/stone.png").unwrap(), "bundled://materials/stone.png");
        assert_eq!(url(Some(&records), "textures/records_only.png").unwrap(), "space://MaterialService/textures/records_only.png");
        assert!(url(None, "textures/records_only.png").is_err(), "the folder does not hold it");
        assert_eq!(url(None, "textures/folder_only.png").unwrap(), "space://MaterialService/textures/folder_only.png");
        assert!(url(Some(&records), "textures/folder_only.png").is_err(), "the records do not hold it");
        for bad in ["../../secret.png", "textures/../../../secret.png", r"\\host\share\x.png", "textures/brick.png:zone"] {
            assert!(url(Some(&records), bad).is_err(), "{bad:?}");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A Space's material files list alike from its folder and from its
    /// records: nested folders walked, hidden files and folders and other
    /// files skipped, in path order (a folder before a longer name that
    /// starts with it).
    #[test]
    fn a_spaces_material_files_list_alike_from_its_folder_and_its_records() {
        let space = std::env::temp_dir().join(format!("eustress_material_files_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&space);
        let mut records = Vec::new();
        for rel in [
            "MaterialService/Brick.mat.toml",
            "MaterialService/A b/Moss.mat.toml",
            "MaterialService/A/Sand.mat.toml",
            "MaterialService/.hidden.mat.toml",
            "MaterialService/.cache/Old.mat.toml",
            "MaterialService/textures/brick.png",
            "Workspace/Stray.mat.toml",
        ] {
            let file = space.join(rel);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, b"[material]\n").unwrap();
            records.push((rel.to_string(), b"[material]\n".to_vec()));
        }
        let (from_records, problems) = source_material_files(&eustress_common::space_source::RecordsSource::new(records));
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            from_records,
            ["MaterialService/A/Sand.mat.toml", "MaterialService/A b/Moss.mat.toml", "MaterialService/Brick.mat.toml"]
        );
        assert_eq!(source_material_files(&eustress_common::space_source::DiskSource::new(&space)), (from_records.clone(), vec![]));
        assert_eq!(
            disk_material_files(&space),
            (from_records.iter().map(|rel| space.join(rel)).collect::<Vec<_>>(), vec![])
        );
        let _ = std::fs::remove_dir_all(&space);
    }

    /// A material folder that cannot be listed is named among the problems
    /// while the rest still list; a Space with no MaterialService reports
    /// nothing. Alike from the folder and from the records.
    #[test]
    fn an_unlistable_material_folder_is_reported_and_a_missing_one_is_not() {
        use eustress_common::space_source::{DiskSource, RecordsSource, SourceEntry};
        /// Records whose `MaterialService/Locked` folder refuses to list.
        struct Locked(RecordsSource);
        impl SpaceSource for Locked {
            fn read(&self, rel: &str) -> std::io::Result<Vec<u8>> {
                self.0.read(rel)
            }
            fn list(&self, rel_dir: &str) -> std::io::Result<Vec<SourceEntry>> {
                if rel_dir == "MaterialService/Locked" {
                    return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "locked"));
                }
                self.0.list(rel_dir)
            }
            fn exists(&self, rel: &str) -> bool {
                self.0.exists(rel)
            }
            fn is_fjall(&self) -> bool {
                false
            }
        }
        let record = |rel: &str| (rel.to_string(), b"[material]\n".to_vec());
        let locked = Locked(RecordsSource::new(vec![
            record("MaterialService/Brick.mat.toml"),
            record("MaterialService/Locked/Moss.mat.toml"),
        ]));
        let (files, problems) = source_material_files(&locked);
        assert_eq!(files, ["MaterialService/Brick.mat.toml"], "the rest still list");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("MaterialService/Locked"), "the folder is named: {}", problems[0]);

        let none = source_material_files(&RecordsSource::new(vec![record("Workspace/Part.toml")]));
        assert_eq!(none, (vec![], vec![]), "no MaterialService, nothing to report");
        let (files, problems) = source_material_files(&RecordsSource::new(vec![record("MaterialService")]));
        assert!(files.is_empty() && problems.len() == 1, "a file where the folder belongs: {problems:?}");

        let base = std::env::temp_dir().join(format!("eustress_material_list_errors_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (bare, odd) = (base.join("bare"), base.join("odd"));
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::create_dir_all(&odd).unwrap();
        std::fs::write(odd.join("MaterialService"), b"not a folder").unwrap();
        assert_eq!(disk_material_files(&bare), (vec![], vec![]), "no MaterialService on disk");
        assert_eq!(source_material_files(&DiskSource::new(&bare)), (vec![], vec![]));
        let (files, problems) = disk_material_files(&odd);
        assert!(files.is_empty() && problems.len() == 1, "a file where the folder belongs: {problems:?}");
        let (files, problems) = source_material_files(&DiskSource::new(&odd));
        assert!(files.is_empty() && problems.len() == 1, "{problems:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    // R2.1 — the dedup key is the scale lever: two parts share ONE
    // Handle<StandardMaterial> (→ Bevy batches them) iff their keys are equal.
    // These pure tests prove the sharing AND copy-on-write logic without a GPU
    // (the engine crate's full build/run is gated by unrelated co-agent errors).

    #[test]
    fn untextured_parts_share_regardless_of_size() {
        // The core ~60K-scale win: untextured solid-color parts of DIFFERENT
        // sizes (→ different uv_transform) must still share. uv is gated on
        // texture, so untextured parts carry no uv in the key → equal.
        let c = Color::srgb(0.2, 0.6, 0.9);
        assert_eq!(
            MaterialCacheKey::new(c, "Plastic", 0.0, 0.0, None),
            MaterialCacheKey::new(c, "Plastic", 0.0, 0.0, None),
        );
    }

    #[test]
    fn distinct_colors_do_not_share() {
        // Copy-on-write: editing a part's color re-resolves it to a DIFFERENT
        // key → different handle; other parts (old color) keep theirs untouched.
        assert_ne!(
            MaterialCacheKey::new(Color::srgb(0.2, 0.6, 0.9), "Plastic", 0.0, 0.0, None),
            MaterialCacheKey::new(Color::srgb(0.9, 0.1, 0.1), "Plastic", 0.0, 0.0, None),
        );
    }

    #[test]
    fn distinct_presets_do_not_share() {
        let c = Color::srgb(0.5, 0.5, 0.5);
        assert_ne!(
            MaterialCacheKey::new(c, "Plastic", 0.0, 0.0, None),
            MaterialCacheKey::new(c, "Glass", 0.0, 0.0, None),
        );
    }

    #[test]
    fn textured_parts_fragment_by_uv() {
        // Textured parts: different tiling is VISIBLE → must NOT share.
        let c = Color::srgb(0.5, 0.5, 0.5);
        let uv_a = bevy::math::Affine2::from_scale(bevy::math::Vec2::new(1.0, 1.0));
        let uv_b = bevy::math::Affine2::from_scale(bevy::math::Vec2::new(5.0, 5.0));
        assert_ne!(
            MaterialCacheKey::new(c, "Brick", 0.0, 0.0, Some(uv_a)),
            MaterialCacheKey::new(c, "Brick", 0.0, 0.0, Some(uv_b)),
        );
        assert_eq!(
            MaterialCacheKey::new(c, "Brick", 0.0, 0.0, Some(uv_a)),
            MaterialCacheKey::new(c, "Brick", 0.0, 0.0, Some(uv_a)),
        );
    }

    #[test]
    fn untextured_and_textured_differ() {
        let c = Color::srgb(0.5, 0.5, 0.5);
        let uv = bevy::math::Affine2::from_scale(bevy::math::Vec2::new(2.0, 2.0));
        assert_ne!(
            MaterialCacheKey::new(c, "Plastic", 0.0, 0.0, None),
            MaterialCacheKey::new(c, "Plastic", 0.0, 0.0, Some(uv)),
        );
    }

    #[test]
    fn library_files_parse_and_superseded_versions_have_successors() {
        for (name, text) in LIBRARY {
            let def = load_material_definition_from_str(text).expect(name);
            assert_eq!(def.material.name, *name);
            assert!(!def.textures.occlusion.is_empty(), "{name}: no occlusion map");
            assert!(current_library_version(text).is_none(), "{name}: current text listed as superseded");
        }
        for (_, name) in SUPERSEDED_LIBRARY_VERSIONS {
            assert!(LIBRARY.iter().any(|(n, _)| n == name), "{name} has no current version");
        }
    }

    #[test]
    fn edited_library_copy_is_used_as_written() {
        let (_, current) = LIBRARY[0];
        let edited = current.replace("reflectance = 0.5", "reflectance = 0.45");
        assert_ne!(edited, current);
        assert!(current_library_version(&edited).is_none());
        let def = load_material_definition_from_str(&edited).unwrap();
        assert_eq!(def.pbr.reflectance, Some(0.45));
    }

    #[test]
    fn transparency_and_reflectance_fragment() {
        let c = Color::srgb(0.5, 0.5, 0.5);
        assert_ne!(
            MaterialCacheKey::new(c, "Plastic", 0.0, 0.0, None),
            MaterialCacheKey::new(c, "Plastic", 0.5, 0.0, None),
        );
        assert_ne!(
            MaterialCacheKey::new(c, "Plastic", 0.0, 0.0, None),
            MaterialCacheKey::new(c, "Plastic", 0.0, 0.5, None),
        );
    }
}

// ============================================================================
// Lazy texture hydration
// ============================================================================

/// Texture maps recorded at material load time but not yet loaded.
#[derive(Debug, Clone)]
pub struct PendingTextures {
    pub refs: TextureProperties,
    pub mat_toml_dir: PathBuf,
    pub space_root: PathBuf,
}

impl MaterialRegistry {
    /// Record `refs` for `name` without loading anything. Called wherever a
    /// material is (re)registered; a re-registration also drops the material
    /// from `hydrated` so a hot-reloaded `.mat.toml` picks up new maps.
    pub fn defer_textures(
        &mut self,
        name: &str,
        refs: &TextureProperties,
        mat_toml_dir: &Path,
        space_root: &Path,
    ) {
        self.hydrated.remove(name);
        let has_any = !(refs.base_color.is_empty()
            && refs.normal.is_empty()
            && refs.metallic_roughness.is_empty()
            && refs.emissive.is_empty()
            && refs.occlusion.is_empty()
            && refs.depth.is_empty());
        if has_any {
            self.pending_textures.insert(
                name.to_string(),
                PendingTextures {
                    refs: refs.clone(),
                    mat_toml_dir: mat_toml_dir.to_path_buf(),
                    space_root: space_root.to_path_buf(),
                },
            );
        } else {
            self.pending_textures.remove(name);
        }
    }

    /// Name of the registry material behind `handle`, if it is one. Linear in
    /// the (small) library size; called only on `Added`/`Changed` part
    /// materials.
    pub fn name_for_handle(&self, handle: &Handle<StandardMaterial>) -> Option<&str> {
        self.materials
            .iter()
            .find(|(_, h)| h.id() == handle.id())
            .map(|(n, _)| n.as_str())
    }

    /// Attach `name`'s texture maps to its `StandardMaterial` if they are
    /// still pending. Idempotent; a no-op for materials without maps.
    pub fn hydrate_textures(
        &mut self,
        name: &str,
        asset_server: &AssetServer,
        materials: &mut Assets<StandardMaterial>,
    ) -> bool {
        if self.hydrated.contains(name) {
            return false;
        }
        let Some(pending) = self.pending_textures.get(name).cloned() else {
            self.hydrated.insert(name.to_string());
            return false;
        };
        let Some(handle) = self.materials.get(name).cloned() else {
            return false;
        };
        let Some(mut mat) = materials.get_mut(&handle) else {
            return false; // asset not resident yet — retry on a later frame
        };
        attach_material_textures(
            &mut *mat,
            &pending.refs,
            asset_server,
            &mut self.texture_cache,
            self.space_source.as_deref(),
            &pending.mat_toml_dir,
            &pending.space_root,
        );
        self.hydrated.insert(name.to_string());
        info!("🎨 material '{}': textures attached on first use", name);
        true
    }
}

/// Load `tex`'s maps and attach them to `mat`. Colour maps (base colour,
/// emissive) are sRGB; every data map (normal, metallic/roughness,
/// occlusion, depth) is linear, and each is mip-filtered the way its data
/// needs (see [`MapKind`]). `space`: where the Space's files are read from
/// when its shell gave a source, `None` for its folder.
pub fn attach_material_textures(
    mat: &mut StandardMaterial,
    tex: &TextureProperties,
    asset_server: &AssetServer,
    cache: &mut HashMap<(String, MapKind), Handle<Image>>,
    space: Option<&dyn SpaceSource>,
    mat_toml_dir: &Path,
    space_root: &Path,
) {
    let mut load = |path: &str, kind: MapKind| -> Option<Handle<Image>> {
        if path.is_empty() {
            return None;
        }
        load_texture(asset_server, cache, space, mat_toml_dir, path, space_root, kind)
    };
    if let Some(h) = load(&tex.base_color, MapKind::Color) {
        mat.base_color_texture = Some(h);
    }
    if let Some(h) = load(&tex.normal, MapKind::Normal) {
        mat.normal_map_texture = Some(h);
    }
    if let Some(h) = load(&tex.metallic_roughness, MapKind::Data) {
        mat.metallic_roughness_texture = Some(h);
    }
    if let Some(h) = load(&tex.emissive, MapKind::Color) {
        mat.emissive_texture = Some(h);
    }
    if let Some(h) = load(&tex.occlusion, MapKind::Data) {
        mat.occlusion_texture = Some(h);
    }
    if let Some(h) = load(&tex.depth, MapKind::Data) {
        mat.depth_map = Some(h);
    }
}

/// Repeat addressing, trilinear filtering, 16× anisotropy: what a tiled
/// surface texture needs at every viewing distance and angle.
fn material_sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..ImageSamplerDescriptor::linear()
    })
}

/// Load a material map GPU-only, with a full mip chain and the material
/// sampler baked in.
///
/// * Mips: PNG and the other formats the `image` crate decodes arrive as a
///   single level, and Bevy builds none, so a map would be sampled at its
///   full 2048² at every distance: shimmer, moiré, and a texel cache thrashed
///   by far-away surfaces. These are read and decoded on the IO pool and
///   given a chain built for their [`MapKind`]. KTX2/DDS/Basis carry their
///   own chain and go through Bevy's loader.
/// * Bytes: read through the asset server's own source for the map's URL
///   (`space://`, `bundled://`), the reader each shell registered, so a
///   map reads alike from a folder and from the browser Player's records,
///   and every shell builds the same chain from it.
/// * `asset_usage = RENDER_WORLD`: no decoded copy stays in `Assets<Image>`
///   after upload (16 MB of RAM per 2048² map that nothing reads again).
/// * The sampler is set here because a `RENDER_WORLD`-only image is never in
///   `Assets<Image>` for anything to patch afterwards.
fn load_image(asset_server: &AssetServer, url: String, kind: MapKind) -> Handle<Image> {
    let ext = Path::new(&url)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if matches!(ext.as_str(), "ktx2" | "dds" | "basis") {
        let is_srgb = kind == MapKind::Color;
        return asset_server
            .load_builder()
            .with_settings(move |s: &mut ImageLoaderSettings| {
                s.is_srgb = is_srgb;
                s.asset_usage = RenderAssetUsages::RENDER_WORLD;
                s.sampler = material_sampler();
            })
            .load(url);
    }
    // The read runs in a task of its own: a source's reader future is not
    // `Send` in a browser, which `add_async` asks of its future, while the
    // task holding it is.
    let server = asset_server.clone();
    let task = bevy::tasks::IoTaskPool::get().spawn(async move {
        let bytes = read_asset_bytes(&server, &url).await;
        decode_with_mips(bytes, &url, &ext, kind)
    });
    asset_server.add_async(async move { Ok::<Image, std::convert::Infallible>(task.await) })
}

/// The bytes of the file an asset URL names, read through the asset server's
/// own source for it.
async fn read_asset_bytes(server: &AssetServer, url: &str) -> Result<Vec<u8>, String> {
    let path = bevy::asset::AssetPath::try_parse(url).map_err(|e| e.to_string())?;
    let source = server.get_source(path.source().clone()).map_err(|e| e.to_string())?;
    let mut reader = source.reader().read(path.path()).await.map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    bevy::asset::io::Reader::read_to_end(&mut *reader, &mut bytes)
        .await
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// Decode a map's bytes and append its mip chain. A map that could not be
/// read or decoded becomes a single neutral texel, so the material still
/// renders (untextured) instead of waiting forever on an image that never
/// arrives. `url` names it in the log.
fn decode_with_mips(bytes: Result<Vec<u8>, String>, url: &str, ext: &str, kind: MapKind) -> Image {
    let decoded = bytes.and_then(|bytes| {
        Image::from_buffer(
            &bytes,
            ImageType::Extension(ext),
            CompressedImageFormats::NONE,
            kind == MapKind::Color,
            material_sampler(),
            RenderAssetUsages::RENDER_WORLD,
        )
        .map_err(|e| e.to_string())
    });
    let mut image = match decoded {
        Ok(image) => image,
        Err(e) => {
            warn!("Material map {} could not be decoded ({}); using a neutral texel", url, e);
            return neutral_texel(kind);
        }
    };
    let desc = &image.texture_descriptor;
    let plain_rgba8 = matches!(desc.format, TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb)
        && desc.mip_level_count == 1
        && desc.dimension == TextureDimension::D2
        && desc.size.depth_or_array_layers == 1;
    if plain_rgba8 {
        let (w, h) = (desc.size.width, desc.size.height);
        if let Some(level0) = image.data.take() {
            let (chain, levels) = build_mip_chain_rgba8(level0, w, h, kind);
            image.data = Some(chain);
            image.texture_descriptor.mip_level_count = levels;
        }
    }
    image
}

/// A 1×1 map that leaves the material's own factors unchanged.
fn neutral_texel(kind: MapKind) -> Image {
    let (texel, format) = match kind {
        MapKind::Color => ([255, 255, 255, 255], TextureFormat::Rgba8UnormSrgb),
        MapKind::Normal => ([128, 128, 255, 255], TextureFormat::Rgba8Unorm),
        MapKind::Data => ([255, 255, 255, 255], TextureFormat::Rgba8Unorm),
    };
    let mut image = Image::new(
        Extent3d::default(),
        TextureDimension::D2,
        texel.to_vec(),
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = material_sampler();
    image
}
