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
    pub emissive: Option<[f32; 4]>,
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
    /// Material preset name (e.g. "Plastic", "Glass", "Neon")
    preset: String,
    /// Transparency quantized to 0-1000 (0.1% precision)
    transparency_millipct: u32,
    /// Reflectance quantized to 0-1000 (0.1% precision)
    reflectance_millipct: u32,
}

impl MaterialCacheKey {
    fn new(color: Color, preset_name: &str, transparency: f32, reflectance: f32) -> Self {
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
        Self {
            color_bits: (r << 24) | (g << 16) | (b << 8) | a,
            preset: preset_name.to_string(),
            transparency_millipct: (transparency.clamp(0.0, 1.0) * 1000.0) as u32,
            reflectance_millipct: (reflectance.clamp(0.0, 1.0) * 1000.0) as u32,
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
}

// ============================================================================
// Parsing and Loading
// ============================================================================

/// Parse a `.mat.toml` file into a MaterialDefinition.
pub fn load_material_definition(path: &Path) -> Result<MaterialDefinition, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    load_material_definition_from_str(&content)
}

/// In-memory twin — parse a `.mat.toml` from content the caller
/// already sourced through `SpaceSource`. No `std::fs`.
pub fn load_material_definition_from_str(content: &str) -> Result<MaterialDefinition, String> {
    toml::from_str::<MaterialDefinition>(content)
        .map_err(|e| format!("Failed to parse material TOML: {}", e))
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
    if let Some([er, eg, eb, ea]) = pbr.emissive {
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

    // Load texture maps if referenced
    let tex = &definition.textures;
    if !tex.base_color.is_empty() {
        if let Some(handle) = load_texture(asset_server, mat_toml_dir, &tex.base_color, space_root) {
            mat.base_color_texture = Some(handle);
        }
    }
    if !tex.normal.is_empty() {
        if let Some(handle) = load_texture(asset_server, mat_toml_dir, &tex.normal, space_root) {
            mat.normal_map_texture = Some(handle);
        }
    }
    if !tex.metallic_roughness.is_empty() {
        if let Some(handle) = load_texture(asset_server, mat_toml_dir, &tex.metallic_roughness, space_root) {
            mat.metallic_roughness_texture = Some(handle);
        }
    }
    if !tex.emissive.is_empty() {
        if let Some(handle) = load_texture(asset_server, mat_toml_dir, &tex.emissive, space_root) {
            mat.emissive_texture = Some(handle);
        }
    }
    if !tex.occlusion.is_empty() {
        if let Some(handle) = load_texture(asset_server, mat_toml_dir, &tex.occlusion, space_root) {
            mat.occlusion_texture = Some(handle);
        }
    }
    if !tex.depth.is_empty() {
        if let Some(handle) = load_texture(asset_server, mat_toml_dir, &tex.depth, space_root) {
            mat.depth_map = Some(handle);
        }
    }

    mat
}

/// Load a texture file relative to the .mat.toml directory via the `space://` asset source.
/// Falls back to bundled engine assets (common/assets/) if not found in user space.
fn load_texture(
    asset_server: &AssetServer,
    mat_toml_dir: &Path,
    relative_path: &str,
    space_root: &Path,
) -> Option<Handle<Image>> {
    // 1. Try relative to the .mat.toml directory (user space)
    let absolute_path = mat_toml_dir.join(relative_path);
    if absolute_path.exists() {
        if let Ok(rel) = absolute_path.strip_prefix(space_root) {
            let asset_path = format!("space://{}", rel.to_string_lossy().replace('\\', "/"));
            return Some(asset_server.load(asset_path));
        } else {
            let asset_path = absolute_path.to_string_lossy().into_owned();
            return Some(asset_server.load(asset_path));
        }
    }

    // 2. Fallback: try bundled engine assets via "bundled://" asset source
    //    e.g. "materials/textures/brick_base_color.png" → "bundled://materials/textures/brick_base_color.png"
    let bundled_check = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../common/assets")
        .join(relative_path);
    if bundled_check.exists() {
        let asset_path = format!("bundled://{}", relative_path.replace('\\', "/"));
        return Some(asset_server.load(asset_path));
    }

    warn!("Texture not found: {:?} (not in space or bundled assets)", absolute_path);
    None
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

    // 2. Dedup cache lookup — share handles for identical (preset, color, transparency)
    let cache_key = MaterialCacheKey::new(base_color, material_name, transparency, reflectance);
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
