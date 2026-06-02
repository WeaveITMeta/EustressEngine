//! Terrain material and shader definitions
//!
//! Provides both StandardMaterial fallback and custom SplatMaterial
//! for multi-layer texture blending based on splatmap.
//!
//! ## Supported Materials (Wave 9.E — full Roblox terrain set)
//!
//! [`TerrainMaterial`] now carries the full ~23 Roblox terrain materials as
//! first-class variants, so imported terrain preserves its true per-cell
//! material identity instead of collapsing onto a handful of buckets.
//!
//! The first 8 variants keep their ORIGINAL discriminant values (Grass=0 …
//! Asphalt=7) — stored voxel/splat data and the importer's
//! `eustress_material` constants depend on those `u8` values, so they are
//! frozen. The Roblox extras are appended at 8..=22:
//!
//! | id | material   | id | material    | id | material   |
//! |----|------------|----|-------------|----|------------|
//! | 0  | Grass      | 8  | Slate       | 16 | Cobblestone|
//! | 1  | Rock       | 9  | Brick       | 17 | Ice        |
//! | 2  | Dirt       | 10 | WoodPlanks  | 18 | LeafyGrass |
//! | 3  | Snow       | 11 | Glacier     | 19 | Salt       |
//! | 4  | Sand       | 12 | Sandstone   | 20 | Limestone  |
//! | 5  | Mud        | 13 | Basalt      | 21 | Pavement   |
//! | 6  | Concrete   | 14 | Ground      | 22 | Water      |
//! | 7  | Asphalt    | 15 | CrackedLava |    |            |
//!
//! ### Roblox cell-id vs. Eustress discriminant
//!
//! NOTE: these Eustress discriminants are NOT the Roblox SmoothGrid cell ids.
//! The importer (`roblox-import/terrain.rs::MATERIAL_TABLE`) translates the
//! Roblox cell-id space (Air=0, Water=1, Grass=2, Slate=3, … Pavement=22)
//! into these Eustress ids and uses two sentinel ids that are NOT terrain-fill
//! variants here: `WATER_MARKER = 254` and `AIR_MARKER = 255`. Eustress keeps
//! [`TerrainMaterial::Water`] as a normal fill variant (id 22) for cells that
//! survive into terrain rather than being lifted into the separate water
//! layer; air is simply the absence of a cell and has no enum variant.
//!
//! ### splat_cache is still 4 layers (renderer is 9.C, not here)
//!
//! `TerrainData.splat_cache` is documented as 4 floats per pixel
//! `[grass, rock, dirt, snow]` and the GPU splat path is not rewritten in this
//! wave. To let the existing 4-layer renderer keep working while the full
//! material identity lives in the voxel data, [`TerrainMaterial::splat_bucket`]
//! maps every one of the 23 materials to one of those 4 indices. Per-material
//! splat is the renderer's job (Wave 9.C) — see that method's docs.

use bevy::prelude::*;

/// Number of distinct [`TerrainMaterial`] variants (Grass=0 … Water=22).
///
/// The discriminants are dense in `0..MATERIAL_COUNT`, so this is both the
/// variant count and `max_discriminant + 1`. Update this if variants change.
pub const MATERIAL_COUNT: usize = 23;

/// Number of splat layers the current renderer blends (Wave 9.C owns the
/// renderer; until it goes per-material the splatmap stays 4-wide
/// `[grass, rock, dirt, snow]`). See [`TerrainMaterial::splat_bucket`].
pub const SPLAT_LAYER_COUNT: usize = 4;

/// Terrain material types for painting.
///
/// The full Roblox terrain material set (Air excluded — it has no fill
/// variant). Discriminants 0..=7 are frozen for stored-data compatibility;
/// see the module docs for the complete id table and the importer's
/// Roblox-cell-id ↔ Eustress-id translation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum TerrainMaterial {
    // ── Original 8 — discriminants FROZEN (stored data depends on them) ──
    #[default]
    Grass = 0,
    Rock = 1,
    Dirt = 2,
    Snow = 3,
    Sand = 4,
    Mud = 5,
    Concrete = 6,
    Asphalt = 7,
    // ── Roblox extras (Wave 9.E) — appended, never renumber the above ──
    Slate = 8,
    Brick = 9,
    WoodPlanks = 10,
    Glacier = 11,
    Sandstone = 12,
    Basalt = 13,
    Ground = 14,
    CrackedLava = 15,
    Cobblestone = 16,
    Ice = 17,
    LeafyGrass = 18,
    Salt = 19,
    Limestone = 20,
    Pavement = 21,
    /// Terrain-fill water cell. Distinct from the importer's `WATER_MARKER`
    /// (254), which tags cells lifted into the separate water layer.
    Water = 22,
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
            Self::Slate => "Slate",
            Self::Brick => "Brick",
            Self::WoodPlanks => "WoodPlanks",
            Self::Glacier => "Glacier",
            Self::Sandstone => "Sandstone",
            Self::Basalt => "Basalt",
            Self::Ground => "Ground",
            Self::CrackedLava => "CrackedLava",
            Self::Cobblestone => "Cobblestone",
            Self::Ice => "Ice",
            Self::LeafyGrass => "LeafyGrass",
            Self::Salt => "Salt",
            Self::Limestone => "Limestone",
            Self::Pavement => "Pavement",
            Self::Water => "Water",
        }
    }

    /// Get base color (linear sRGB) for this material.
    ///
    /// Plausible PBR albedos chosen so every variant is visually distinct;
    /// the importer's per-cell `MaterialColors` override (when present) takes
    /// precedence at render time, these are the defaults.
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
            // Slate: blue-grey rock.
            Self::Slate => Color::srgb(0.34, 0.36, 0.40),
            // Brick: classic terracotta red.
            Self::Brick => Color::srgb(0.55, 0.27, 0.20),
            // WoodPlanks: warm mid-brown timber.
            Self::WoodPlanks => Color::srgb(0.52, 0.36, 0.20),
            // Glacier: pale blue-white ice (brighter/bluer than snow).
            Self::Glacier => Color::srgb(0.80, 0.88, 0.95),
            // Sandstone: pale tan, lighter than sand.
            Self::Sandstone => Color::srgb(0.80, 0.68, 0.50),
            // Basalt: near-black volcanic grey.
            Self::Basalt => Color::srgb(0.16, 0.16, 0.18),
            // Ground: neutral earthy brown (between dirt and mud).
            Self::Ground => Color::srgb(0.45, 0.36, 0.27),
            // CrackedLava: dark crust with a hot orange tint.
            Self::CrackedLava => Color::srgb(0.30, 0.13, 0.08),
            // Cobblestone: cool mid-grey stone.
            Self::Cobblestone => Color::srgb(0.45, 0.43, 0.42),
            // Ice: bright cyan-white, near-white.
            Self::Ice => Color::srgb(0.78, 0.90, 0.96),
            // LeafyGrass: slightly richer/darker green than Grass.
            Self::LeafyGrass => Color::srgb(0.28, 0.50, 0.20),
            // Salt: bright off-white crystalline.
            Self::Salt => Color::srgb(0.92, 0.92, 0.88),
            // Limestone: pale warm grey.
            Self::Limestone => Color::srgb(0.74, 0.72, 0.66),
            // Pavement: medium grey, a touch lighter than concrete.
            Self::Pavement => Color::srgb(0.55, 0.55, 0.57),
            // Water: deep blue-green (fill cells; the water layer renders separately).
            Self::Water => Color::srgb(0.10, 0.30, 0.45),
        }
    }

    /// Get perceptual roughness for this material (0 = mirror, 1 = fully matte).
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
            Self::Slate => 0.55,        // smooth split stone
            Self::Brick => 0.8,
            Self::WoodPlanks => 0.7,
            Self::Glacier => 0.25,      // glassy ice — low roughness
            Self::Sandstone => 0.9,
            Self::Basalt => 0.7,
            Self::Ground => 0.9,
            Self::CrackedLava => 0.85,  // rough crust
            Self::Cobblestone => 0.8,
            Self::Ice => 0.12,          // near-mirror
            Self::LeafyGrass => 0.85,
            Self::Salt => 0.7,
            Self::Limestone => 0.85,
            Self::Pavement => 0.72,
            Self::Water => 0.05,        // smooth/reflective fill water
        }
    }

    /// Get metallic factor for this material. Terrain is overwhelmingly
    /// dielectric, so this is 0 everywhere; kept as a method so the renderer
    /// has a single source of truth and exotic materials can opt in later.
    pub fn metallic(&self) -> f32 {
        match self {
            // All current terrain materials are non-metallic.
            Self::Grass | Self::Rock | Self::Dirt | Self::Snow | Self::Sand
            | Self::Mud | Self::Concrete | Self::Asphalt | Self::Slate
            | Self::Brick | Self::WoodPlanks | Self::Glacier | Self::Sandstone
            | Self::Basalt | Self::Ground | Self::CrackedLava | Self::Cobblestone
            | Self::Ice | Self::LeafyGrass | Self::Salt | Self::Limestone
            | Self::Pavement | Self::Water => 0.0,
        }
    }

    /// Splat-layer bucket for this material.
    ///
    /// The renderer's `splat_cache` is still 4-wide (Wave 9.C owns making it
    /// per-material). Until then, every one of the 23 materials collapses to
    /// one of the 4 existing splat layers so the current blend keeps working
    /// visually while the full material id is preserved losslessly in the
    /// voxel data:
    ///
    /// | bucket | layer | gathers                                            |
    /// |--------|-------|----------------------------------------------------|
    /// | 0      | Grass | Grass, LeafyGrass                                  |
    /// | 1      | Rock  | Rock, Slate, Basalt, CrackedLava, Cobblestone, Limestone, Brick, Concrete, Asphalt, Pavement, WoodPlanks |
    /// | 2      | Dirt  | Dirt, Mud, Ground, Sand, Sandstone                 |
    /// | 3      | Snow  | Snow, Glacier, Ice, Salt, Water                    |
    ///
    /// TODO(Wave 9.C): replace this 4-bucket clamp with a per-material splat
    /// weight once the GPU splat path widens beyond `[grass,rock,dirt,snow]`.
    pub fn splat_bucket(&self) -> usize {
        match self {
            // 0 — grass-ish (green vegetation)
            Self::Grass | Self::LeafyGrass => 0,
            // 1 — rock/hard-surface (stone, masonry, paving, planks)
            Self::Rock | Self::Slate | Self::Basalt | Self::CrackedLava
            | Self::Cobblestone | Self::Limestone | Self::Brick | Self::Concrete
            | Self::Asphalt | Self::Pavement | Self::WoodPlanks => 1,
            // 2 — dirt/sand-ish (loose earthy ground)
            Self::Dirt | Self::Mud | Self::Ground | Self::Sand | Self::Sandstone => 2,
            // 3 — snow/ice-ish (bright, high-reflect, cold)
            Self::Snow | Self::Glacier | Self::Ice | Self::Salt | Self::Water => 3,
        }
    }

    /// Get all material types
    pub fn all() -> &'static [TerrainMaterial] {
        &[
            Self::Grass, Self::Rock, Self::Dirt, Self::Snow,
            Self::Sand, Self::Mud, Self::Concrete, Self::Asphalt,
            Self::Slate, Self::Brick, Self::WoodPlanks, Self::Glacier,
            Self::Sandstone, Self::Basalt, Self::Ground, Self::CrackedLava,
            Self::Cobblestone, Self::Ice, Self::LeafyGrass, Self::Salt,
            Self::Limestone, Self::Pavement, Self::Water,
        ]
    }

    /// This material's `u8` discriminant (the value stored in voxel data).
    #[inline]
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    /// Construct from a stored `u8` discriminant.
    ///
    /// Returns `None` for ids outside `0..MATERIAL_COUNT` (including the
    /// importer's `WATER_MARKER = 254` / `AIR_MARKER = 255` sentinels, which
    /// are not terrain-fill variants). Use [`Self::from_u8_or_default`] when a
    /// total mapping is wanted.
    pub fn from_u8(id: u8) -> Option<Self> {
        Some(match id {
            0 => Self::Grass,
            1 => Self::Rock,
            2 => Self::Dirt,
            3 => Self::Snow,
            4 => Self::Sand,
            5 => Self::Mud,
            6 => Self::Concrete,
            7 => Self::Asphalt,
            8 => Self::Slate,
            9 => Self::Brick,
            10 => Self::WoodPlanks,
            11 => Self::Glacier,
            12 => Self::Sandstone,
            13 => Self::Basalt,
            14 => Self::Ground,
            15 => Self::CrackedLava,
            16 => Self::Cobblestone,
            17 => Self::Ice,
            18 => Self::LeafyGrass,
            19 => Self::Salt,
            20 => Self::Limestone,
            21 => Self::Pavement,
            22 => Self::Water,
            _ => return None,
        })
    }

    /// Construct from a stored `u8`, falling back to the default
    /// ([`TerrainMaterial::Grass`]) for unknown ids — useful when decoding
    /// untrusted voxel data where an out-of-range id should not abort.
    #[inline]
    pub fn from_u8_or_default(id: u8) -> Self {
        Self::from_u8(id).unwrap_or_default()
    }

    /// Convert from a dense layer index (`0..MATERIAL_COUNT`), saturating to
    /// the default for out-of-range indices.
    ///
    /// Equivalent to [`Self::from_u8_or_default`] for `layer < 256`; kept for
    /// the existing `usize`-indexed `blend_materials` call site.
    pub fn from_layer(layer: usize) -> Self {
        u8::try_from(layer)
            .ok()
            .and_then(Self::from_u8)
            .unwrap_or_default()
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

/// Blend the first 8 materials (by discriminant) according to per-layer
/// weights — a legacy CPU fallback helper.
///
/// `weights[i]` weights `TerrainMaterial::from_layer(i)`, so this covers
/// discriminants 0..=7 (Grass..=Asphalt). The full 23-material identity lives
/// in the voxel data; widening this blend to all materials is the renderer's
/// job (Wave 9.C). Kept for the existing CPU vertex-color path.
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

// ---------------------------------------------------------------------------
// Tests (Wave 9.E — full 23-material set)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_returns_every_variant_once() {
        let all = TerrainMaterial::all();
        assert_eq!(
            all.len(),
            MATERIAL_COUNT,
            "TerrainMaterial::all() must list all {MATERIAL_COUNT} variants"
        );
        // No duplicates (compare by discriminant).
        let mut ids: Vec<u8> = all.iter().map(|m| m.to_u8()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), MATERIAL_COUNT, "all() contains a duplicate variant");
        // Discriminants are dense 0..MATERIAL_COUNT.
        assert_eq!(*ids.first().unwrap(), 0);
        assert_eq!(*ids.last().unwrap(), (MATERIAL_COUNT - 1) as u8);
    }

    #[test]
    fn every_variant_has_a_nonempty_name() {
        for m in TerrainMaterial::all() {
            assert!(
                !m.name().is_empty(),
                "{m:?} (id {}) has an empty name",
                m.to_u8()
            );
        }
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = TerrainMaterial::all().iter().map(|m| m.name()).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "two materials share a name");
    }

    #[test]
    fn base_colors_are_distinct_ish() {
        // Every variant should have a recognisably different albedo. Compare
        // each pair; require at least a small channel-sum difference so no two
        // materials are byte-identical (would defeat per-material identity).
        let all = TerrainMaterial::all();
        for (i, a) in all.iter().enumerate() {
            let ca = a.base_color().to_srgba();
            for b in &all[i + 1..] {
                let cb = b.base_color().to_srgba();
                let diff = (ca.red - cb.red).abs()
                    + (ca.green - cb.green).abs()
                    + (ca.blue - cb.blue).abs();
                assert!(
                    diff > 0.01,
                    "{a:?} and {b:?} have near-identical base colors (Δ={diff:.4})"
                );
            }
        }
    }

    #[test]
    fn roughness_in_unit_range() {
        for m in TerrainMaterial::all() {
            let r = m.roughness();
            assert!(
                (0.0..=1.0).contains(&r),
                "{m:?} roughness {r} out of [0,1]"
            );
        }
    }

    #[test]
    fn from_u8_round_trips_all_variants() {
        for m in TerrainMaterial::all() {
            let id = m.to_u8();
            assert_eq!(
                TerrainMaterial::from_u8(id),
                Some(*m),
                "round-trip failed for {m:?} (id {id})"
            );
        }
    }

    #[test]
    fn from_u8_rejects_out_of_range_and_sentinels() {
        // First invalid id is MATERIAL_COUNT.
        assert_eq!(TerrainMaterial::from_u8(MATERIAL_COUNT as u8), None);
        // The importer's sentinels are NOT fill variants.
        assert_eq!(TerrainMaterial::from_u8(254), None, "WATER_MARKER must not decode");
        assert_eq!(TerrainMaterial::from_u8(255), None, "AIR_MARKER must not decode");
        // Total fallback still yields a valid material.
        assert_eq!(TerrainMaterial::from_u8_or_default(254), TerrainMaterial::default());
        assert_eq!(TerrainMaterial::from_u8_or_default(255), TerrainMaterial::Grass);
    }

    #[test]
    fn from_layer_matches_from_u8_and_saturates() {
        for i in 0..MATERIAL_COUNT {
            assert_eq!(
                TerrainMaterial::from_layer(i),
                TerrainMaterial::from_u8(i as u8).unwrap()
            );
        }
        // Out-of-range layer indices fall back to the default.
        assert_eq!(TerrainMaterial::from_layer(MATERIAL_COUNT), TerrainMaterial::default());
        assert_eq!(TerrainMaterial::from_layer(99_999), TerrainMaterial::default());
    }

    #[test]
    fn splat_bucket_always_in_range() {
        for m in TerrainMaterial::all() {
            let b = m.splat_bucket();
            assert!(
                b < SPLAT_LAYER_COUNT,
                "{m:?} splat_bucket {b} >= {SPLAT_LAYER_COUNT}"
            );
        }
    }

    #[test]
    fn splat_buckets_align_with_legacy_four_layers() {
        // The 4-layer splat order is [grass, rock, dirt, snow]; the original
        // four materials must still map to their own slot so the existing
        // renderer is unchanged for legacy terrain.
        assert_eq!(TerrainMaterial::Grass.splat_bucket(), 0);
        assert_eq!(TerrainMaterial::Rock.splat_bucket(), 1);
        assert_eq!(TerrainMaterial::Dirt.splat_bucket(), 2);
        assert_eq!(TerrainMaterial::Snow.splat_bucket(), 3);
        // A few representative new materials land in sensible buckets.
        assert_eq!(TerrainMaterial::LeafyGrass.splat_bucket(), 0);
        assert_eq!(TerrainMaterial::Basalt.splat_bucket(), 1);
        assert_eq!(TerrainMaterial::Sand.splat_bucket(), 2);
        assert_eq!(TerrainMaterial::Ice.splat_bucket(), 3);
    }

    #[test]
    fn discriminants_match_importer_eustress_material_constants() {
        // These MUST stay in lockstep with roblox-import's `eustress_material`
        // module so the importer's u8s decode to the right variant here.
        assert_eq!(TerrainMaterial::Grass as u8, 0);
        assert_eq!(TerrainMaterial::Rock as u8, 1);
        assert_eq!(TerrainMaterial::Dirt as u8, 2);
        assert_eq!(TerrainMaterial::Snow as u8, 3);
        assert_eq!(TerrainMaterial::Sand as u8, 4);
        assert_eq!(TerrainMaterial::Mud as u8, 5);
        assert_eq!(TerrainMaterial::Concrete as u8, 6);
        assert_eq!(TerrainMaterial::Asphalt as u8, 7);
        // And the appended set is contiguous through Water=22.
        assert_eq!(TerrainMaterial::Water as u8, 22);
    }
}
