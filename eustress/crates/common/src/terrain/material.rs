//! Terrain material and shader definitions
//!
//! Provides both StandardMaterial fallback and custom SplatMaterial
//! for multi-layer texture blending based on splatmap.
//!
//! ## Supported Materials
//! - Grass, Rock, Dirt, Snow (original 4)
//! - Sand, Mud, Concrete, Asphalt (extended 4)

use bevy::prelude::*;

/// Terrain material types for painting
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum TerrainMaterial {
    #[default]
    Grass = 0,
    Rock = 1,
    Dirt = 2,
    Snow = 3,
    Sand = 4,
    Mud = 5,
    Concrete = 6,
    Asphalt = 7,
}

impl TerrainMaterial {
    /// Get material name
    pub fn name(&self) -> &'static str {
        match self {
            Self::Grass => "Grass",
            Self::Rock => "Rock",
            Self::Dirt => "Dirt",
            Self::Snow => "Snow",
            Self::Sand => "Sand",
            Self::Mud => "Mud",
            Self::Concrete => "Concrete",
            Self::Asphalt => "Asphalt",
        }
    }
    
    /// Get base color for this material
    pub fn base_color(&self) -> Color {
        match self {
            Self::Grass => Color::srgb(0.35, 0.55, 0.25),
            Self::Rock => Color::srgb(0.5, 0.45, 0.4),
            Self::Dirt => Color::srgb(0.55, 0.4, 0.3),
            Self::Snow => Color::srgb(0.95, 0.95, 0.98),
            Self::Sand => Color::srgb(0.76, 0.70, 0.50),
            Self::Mud => Color::srgb(0.35, 0.25, 0.15),
            Self::Concrete => Color::srgb(0.6, 0.6, 0.6),
            Self::Asphalt => Color::srgb(0.2, 0.2, 0.22),
        }
    }
    
    /// Get roughness for this material
    pub fn roughness(&self) -> f32 {
        match self {
            Self::Grass => 0.85,
            Self::Rock => 0.75,
            Self::Dirt => 0.9,
            Self::Snow => 0.6,
            Self::Sand => 0.95,
            Self::Mud => 0.8,
            Self::Concrete => 0.7,
            Self::Asphalt => 0.65,
        }
    }
    
    /// Get all material types
    pub fn all() -> &'static [TerrainMaterial] {
        &[
            Self::Grass, Self::Rock, Self::Dirt, Self::Snow,
            Self::Sand, Self::Mud, Self::Concrete, Self::Asphalt,
        ]
    }
    
    /// Convert from layer index
    pub fn from_layer(layer: usize) -> Self {
        match layer {
            0 => Self::Grass,
            1 => Self::Rock,
            2 => Self::Dirt,
            3 => Self::Snow,
            4 => Self::Sand,
            5 => Self::Mud,
            6 => Self::Concrete,
            7 => Self::Asphalt,
            _ => Self::Grass,
        }
    }
}

/// Terrain material configuration for splat-based texturing
#[derive(Clone, Debug)]
pub struct TerrainMaterialConfig {
    /// Textures for each material layer (up to 8)
    pub textures: [Option<Handle<Image>>; 8],
    
    /// Splatmap for blending (2 RGBA textures for 8 layers)
    pub splatmap: Option<Handle<Image>>,
    pub splatmap2: Option<Handle<Image>>,
    
    /// Tiling factor for textures
    pub texture_scale: f32,
}

impl Default for TerrainMaterialConfig {
    fn default() -> Self {
        Self {
            textures: Default::default(),
            splatmap: None,
            splatmap2: None,
            texture_scale: 10.0,
        }
    }
}

/// Create a basic terrain material using StandardMaterial
/// Use this as fallback when custom shaders aren't needed
pub fn create_terrain_material(
    materials: &mut Assets<StandardMaterial>,
    _config: &TerrainMaterialConfig,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.55, 0.25),  // Grass green
        perceptual_roughness: 0.85,
        metallic: 0.0,
        reflectance: 0.2,
        ..default()
    })
}

/// Create height-based terrain material with color gradient
pub fn create_height_gradient_material(
    materials: &mut Assets<StandardMaterial>,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::srgb(0.4, 0.6, 0.3),  // Base grass
        perceptual_roughness: 0.8,
        metallic: 0.0,
        reflectance: 0.25,
        ..default()
    })
}

/// Height-based material blending parameters
#[derive(Clone, Debug)]
pub struct HeightBlendParams {
    /// Height threshold for grass -> rock transition
    pub grass_to_rock: f32,
    
    /// Height threshold for rock -> snow transition
    pub rock_to_snow: f32,
    
    /// Blend range for smooth transitions
    pub blend_range: f32,
    
    /// Slope threshold for rock (radians)
    pub slope_rock_threshold: f32,
}

impl Default for HeightBlendParams {
    fn default() -> Self {
        Self {
            grass_to_rock: 20.0,
            rock_to_snow: 50.0,
            blend_range: 5.0,
            slope_rock_threshold: 0.7,  // ~40 degrees
        }
    }
}

/// Calculate splat weights based on height and slope
/// Returns [grass, rock, dirt, snow] weights (sum to 1.0)
pub fn calculate_splat_weights(height: f32, slope: f32, params: &HeightBlendParams) -> [f32; 4] {
    let mut weights = [0.0f32; 4];
    
    // Slope-based rock blending
    let slope_factor = (slope / params.slope_rock_threshold).clamp(0.0, 1.0);
    
    // Height-based layer selection
    if height < params.grass_to_rock - params.blend_range {
        // Pure grass zone
        weights[0] = 1.0 - slope_factor;  // Grass
        weights[1] = slope_factor;         // Rock on slopes
    } else if height < params.grass_to_rock + params.blend_range {
        // Grass to rock transition
        let t = (height - (params.grass_to_rock - params.blend_range)) / (params.blend_range * 2.0);
        weights[0] = (1.0 - t) * (1.0 - slope_factor);
        weights[1] = t + slope_factor * (1.0 - t);
    } else if height < params.rock_to_snow - params.blend_range {
        // Pure rock zone
        weights[1] = 1.0;
    } else if height < params.rock_to_snow + params.blend_range {
        // Rock to snow transition
        let t = (height - (params.rock_to_snow - params.blend_range)) / (params.blend_range * 2.0);
        weights[1] = 1.0 - t;
        weights[3] = t;
    } else {
        // Pure snow zone
        weights[3] = 1.0;
    }
    
    // Normalize weights to sum to 1.0
    let sum: f32 = weights.iter().sum();
    if sum > 0.0 {
        for w in &mut weights {
            *w /= sum;
        }
    } else {
        weights[0] = 1.0;  // Default to grass
    }
    
    weights
}

/// Get color for height (for vertex coloring fallback)
pub fn height_to_color(height: f32, params: &HeightBlendParams) -> Color {
    let weights = calculate_splat_weights(height, 0.0, params);
    
    // Get colors from TerrainMaterial enum
    let colors: [Vec3; 4] = [
        color_to_vec3(TerrainMaterial::Grass.base_color()),
        color_to_vec3(TerrainMaterial::Rock.base_color()),
        color_to_vec3(TerrainMaterial::Dirt.base_color()),
        color_to_vec3(TerrainMaterial::Snow.base_color()),
    ];
    
    let color = colors[0] * weights[0] + colors[1] * weights[1] + colors[2] * weights[2] + colors[3] * weights[3];
    Color::srgb(color.x, color.y, color.z)
}

/// Convert Color to Vec3 for blending
fn color_to_vec3(color: Color) -> Vec3 {
    let srgba = color.to_srgba();
    Vec3::new(srgba.red, srgba.green, srgba.blue)
}

/// Get color for a specific material layer
pub fn material_to_color(material: TerrainMaterial) -> Color {
    material.base_color()
}

/// Blend multiple materials based on weights (up to 8 layers)
pub fn blend_materials(weights: &[f32; 8]) -> Color {
    let mut color = Vec3::ZERO;
    let mut total_weight = 0.0;
    
    for (i, &weight) in weights.iter().enumerate() {
        if weight > 0.0 {
            let mat = TerrainMaterial::from_layer(i);
            color += color_to_vec3(mat.base_color()) * weight;
            total_weight += weight;
        }
    }
    
    if total_weight > 0.0 {
        color /= total_weight;
    } else {
        color = color_to_vec3(TerrainMaterial::Grass.base_color());
    }
    
    Color::srgb(color.x, color.y, color.z)
}

/// Terrain shader constants (for future custom material)
pub mod shader {
    /// Vertex shader for terrain (placeholder - uses default PBR)
    pub const TERRAIN_VERTEX: &str = r#"
        // Standard PBR vertex shader
        // Custom displacement could be added here
    "#;
    
    /// Fragment shader for terrain splat blending (placeholder)
    pub const TERRAIN_FRAGMENT: &str = r#"
        // Splat-based texture blending
        // Sample 4 textures and blend based on splatmap
    "#;
}
