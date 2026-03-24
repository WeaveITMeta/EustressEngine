//! # Material Loader
//!
//! Parses `.mat.toml` files from MaterialService into Bevy `StandardMaterial` handles.
//! Maintains a `MaterialRegistry` resource mapping material names to cached handles.
//!
//! ## Resolution Order (for Parts)
//! 1. MaterialRegistry — exact name match against loaded `.mat.toml` files
//! 2. Material enum fallback — `Material::from_string()` for 18 built-in presets
//! 3. Default — `Material::Plastic`

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
    pub textures: TextureReferences,
}

/// [material] section — name, preset, description
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MaterialMetadata {
    /// Display name (defaults to filename stem if empty)
    #[serde(default)]
    pub name: String,
    /// Optional preset — inherit PBR defaults from Material enum variant
    #[serde(default)]
    pub preset: Option<String>,
    /// Human-readable description
    #[serde(default)]
    pub description: String,
    /// Searchable tags
    #[serde(default)]
    pub tags: Vec<String>,
}

/// [pbr] section — scalar PBR properties for StandardMaterial
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PbrProperties {
    /// Base color RGBA linear [0.0–1.0]
    #[serde(default = "default_base_color")]
    pub base_color: [f32; 4],
    /// 0.0 = dielectric, 1.0 = metal
    #[serde(default)]
    pub metallic: Option<f32>,
    /// 0.0 = mirror, 1.0 = matte
    #[serde(default)]
    pub roughness: Option<f32>,
    /// Fresnel reflectance at normal incidence
    #[serde(default)]
    pub reflectance: Option<f32>,
    /// Emissive color RGBA
    #[serde(default)]
    pub emissive: Option<[f32; 4]>,
    /// "Opaque" | "Blend" | "Mask"
    #[serde(default = "default_alpha_mode")]
    pub alpha_mode: String,
    /// Alpha cutoff for Mask mode
    #[serde(default = "default_alpha_cutoff")]
    pub alpha_cutoff: f32,
    /// Index of refraction
    #[serde(default)]
    pub ior: Option<f32>,
    /// Specular transmission (glass-like)
    #[serde(default)]
    pub specular_transmission: Option<f32>,
    /// Diffuse transmission (thin translucent)
    #[serde(default)]
    pub diffuse_transmission: Option<f32>,
    /// Transmission thickness
    #[serde(default)]
    pub thickness: Option<f32>,
    /// Render both faces
    #[serde(default)]
    pub double_sided: bool,
    /// Skip PBR lighting (for Neon-like glow)
    #[serde(default)]
    pub unlit: bool,
}

fn default_base_color() -> [f32; 4] {
    [0.5, 0.5, 0.5, 1.0]
}

fn default_alpha_mode() -> String {
    "Opaque".to_string()
}

fn default_alpha_cutoff() -> f32 {
    0.5
}

impl Default for PbrProperties {
    fn default() -> Self {
        Self {
            base_color: default_base_color(),
            metallic: None,
            roughness: None,
            reflectance: None,
            emissive: None,
            alpha_mode: default_alpha_mode(),
            alpha_cutoff: default_alpha_cutoff(),
            ior: None,
            specular_transmission: None,
            diffuse_transmission: None,
            thickness: None,
            double_sided: false,
            unlit: false,
        }
    }
}

/// [textures] section — relative paths to texture image files
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TextureReferences {
    /// Albedo / base color map
    #[serde(default)]
    pub base_color: String,
    /// Normal map
    #[serde(default)]
    pub normal: String,
    /// Combined metallic (B) / roughness (G) map (glTF convention)
    #[serde(default)]
    pub metallic_roughness: String,
    /// Emissive map
    #[serde(default)]
    pub emissive: String,
    /// Ambient occlusion map
    #[serde(default)]
    pub occlusion: String,
    /// Depth / parallax / displacement map
    #[serde(default)]
    pub depth: String,
}

// ============================================================================
// MaterialRegistry — central material cache resource
// ============================================================================

/// Central cache mapping material names to Bevy material handles.
/// Populated on Space load from `MaterialService/*.mat.toml` files.
#[derive(Resource, Default)]
pub struct MaterialRegistry {
    /// Name → loaded Bevy material handle
    materials: HashMap<String, Handle<StandardMaterial>>,
    /// Name → parsed definition (for future property panel editing)
    definitions: HashMap<String, MaterialDefinition>,
    /// Name → source .mat.toml path (for writeback and hot-reload)
    source_paths: HashMap<String, PathBuf>,
    /// Deduplication cache: quantized material parameters → shared handle.
    /// Entities with identical visual parameters share one GPU material,
    /// enabling Bevy's automatic batching to merge draw calls.
    dedup_cache: HashMap<MaterialCacheKey, Handle<StandardMaterial>>,
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
    /// Build a cache key from the same parameters resolve_material receives.
    fn new(base_color: Color, preset_name: &str, transparency: f32, reflectance: f32) -> Self {
        let srgba = base_color.to_srgba();
        let r = (srgba.red.clamp(0.0, 1.0) * 255.0) as u32;
        let g = (srgba.green.clamp(0.0, 1.0) * 255.0) as u32;
        let b = (srgba.blue.clamp(0.0, 1.0) * 255.0) as u32;
        let a = (srgba.alpha.clamp(0.0, 1.0) * 255.0) as u32;
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
    pub fn dedup_get_or_insert(
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

    let mut definition: MaterialDefinition = toml::from_str(&content)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;

    // Default name to filename stem if not set
    if definition.material.name.is_empty() {
        definition.material.name = material_name_from_path(path);
    }

    Ok(definition)
}

/// Extract material name from a `.mat.toml` path.
/// Example: `RustyMetal.mat.toml` → `"RustyMetal"`
pub fn material_name_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("Unknown")
        .strip_suffix(".mat.toml")
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
        })
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
fn load_texture(
    asset_server: &AssetServer,
    mat_toml_dir: &Path,
    relative_path: &str,
    space_root: &Path,
) -> Option<Handle<Image>> {
    let absolute_path = mat_toml_dir.join(relative_path);
    if !absolute_path.exists() {
        warn!("Texture not found: {:?} (referenced from material)", absolute_path);
        return None;
    }
    // Convert to space-relative path for AssetServer
    let space_relative = absolute_path
        .strip_prefix(space_root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| absolute_path.to_string_lossy().replace('\\', "/"));

    let asset_path = format!("space://{}", space_relative);
    Some(asset_server.load(asset_path))
}

// ============================================================================
// Resolve material for a Part — registry-first, enum fallback
// ============================================================================

/// Resolve a material name to a `Handle<StandardMaterial>`.
///
/// 1. Check `MaterialRegistry` for a loaded `.mat.toml` handle
/// 2. Fall back to creating a `StandardMaterial` from the `Material` enum preset
/// 3. Default to `Material::Plastic`
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

    // 2. Dedup cache lookup — return existing handle if same visual params
    let cache_key = MaterialCacheKey::new(base_color, material_name, transparency, reflectance);
    if let Some(handle) = registry.dedup_cache.get(&cache_key) {
        return handle.clone();
    }

    // 3. Enum fallback — create from Material preset scalars
    let preset = PresetMaterial::from_string(material_name);
    let (roughness, metallic, preset_reflectance) = preset.pbr_params();

    let alpha = base_color.alpha() * (1.0 - transparency);
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
        mat.emissive = LinearRgba::from(base_color) * 2.0;
    }

    let handle = materials.add(mat);
    // Cache for future entities with identical visual parameters
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
        eustress_common::classes::Instance {
            name: name.clone(),
            class_name: eustress_common::classes::ClassName::Folder, // Use Folder until MaterialDefinition ClassName is added
            archivable: true,
            id: 0,
            ai: false,
        },
        MaterialDefinitionComponent {
            name: name.clone(),
            source_path: path.clone(),
        },
        super::file_loader::LoadedFromFile {
            path: path.clone(),
            file_type: super::file_loader::FileType::Material,
            service: "MaterialService".to_string(),
        },
        Name::new(name),
        Transform::default(),
        Visibility::default(),
    )).id()
}
