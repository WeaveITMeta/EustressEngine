//! Terrain material identity and material definitions
//!
//! The 23 built-in terrain materials, the per-cell material-map encoding the
//! heightfield stores them in, and the height-band colours procedural
//! terrain (which has no material map) falls back to. How each slot is
//! drawn lives in `material_slots`, and the textured material that draws the
//! map in `surface_material`.
//!
//! ## Supported Materials (Wave 9.E — full Roblox terrain set)
//!
//! [`TerrainMaterial`] now carries the full ~23 Roblox terrain materials as
//! first-class variants, so imported terrain preserves its true per-cell
//! material identity instead of collapsing onto a handful of buckets.
//!
//! The first 8 variants keep their ORIGINAL discriminant values (Grass=0 …
//! Asphalt=7): stored voxel and material-map data and the importer's
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
//! ## Material slots and the material map
//!
//! The heightfield carries per-cell material identity in
//! `TerrainData.material_cache`: one [`MaterialCell`] per height sample,
//! `[id_a, id_b, blend_b, reserved]`, where `id_a` and `id_b` are material
//! SLOTS and `blend_b` is the weight of `id_b` in 1/255 steps. Slots
//! `0..MATERIAL_COUNT` are the built-in [`TerrainMaterial`] variants by
//! discriminant, so a stored cell names the same material the voxel data
//! does. Slots `MATERIAL_COUNT..=254` are a Space's custom materials, and
//! [`MATERIAL_SLOT_NONE`] (255) is "no material". A cell holding one
//! material is `[slot, 255, 0, 0]`; writers keep the stronger material in
//! `id_a` (see [`canonical_material_cell`]), readers accept any `blend_b`.
//!
//! On disk the same cells are `Workspace/Terrain/matmap/x{cx}_z{cz}.png`,
//! RGBA8, one pixel per cell. Every terrain writer (Save, the worldgen and
//! flat exporters, the heightmap importer) emits a matmap; the old 4-bucket
//! `splatmap/*.png` is only ever read, converted by
//! [`legacy_splat_to_material_cell`], and removed by the next Save.

use bevy::prelude::*;

use super::TerrainConfig;

/// Number of distinct [`TerrainMaterial`] variants (Grass=0 … Water=22).
///
/// The discriminants are dense in `0..MATERIAL_COUNT`, so this is both the
/// variant count and `max_discriminant + 1`. Update this if variants change.
/// It is also the first custom material slot: slots below it are the
/// built-in variants.
pub const MATERIAL_COUNT: usize = 23;

/// Slot ids a material-map cell can name: a `u8`, so 256, of which
/// [`MATERIAL_SLOT_NONE`] is reserved.
pub const MATERIAL_SLOT_COUNT: usize = 256;

/// The slot id meaning "no material": an unused `id_b`, or a cell no writer
/// has given a material yet (the voxel loader's air columns). Readers skip
/// it, and a cell with no material at all renders as Grass.
pub const MATERIAL_SLOT_NONE: u8 = 255;

/// First slot id of a Space's custom materials.
pub const FIRST_CUSTOM_MATERIAL_SLOT: u8 = MATERIAL_COUNT as u8;

/// One material-map cell: `[id_a, id_b, blend_b, reserved]`. See the module
/// docs for the encoding.
pub type MaterialCell = [u8; 4];

/// Fraction of the height band, measured up from `height_offset`, at and
/// above which the legacy splatmap's fourth bucket (snow, glacier, ice, salt
/// and water together) means Snow; below it, Water. The rule the old
/// vertex-colour mesher applied per vertex, kept so converted Spaces look
/// as they did.
pub const LEGACY_SNOW_ALTITUDE_FRACTION: f32 = 0.72;

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

    /// The material named `name`, ignoring case, spaces, underscores and
    /// hyphens, so "WoodPlanks", "wood planks" and "wood_planks" all match.
    /// `None` for a name that is not one of the 23 variants.
    pub fn from_name(name: &str) -> Option<Self> {
        let squash = |s: &str| -> String {
            s.chars()
                .filter(|&c| !matches!(c, ' ' | '_' | '-'))
                .flat_map(char::to_lowercase)
                .collect()
        };
        let wanted = squash(name);
        Self::all().iter().copied().find(|m| squash(m.name()) == wanted)
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
}

// ============================================================================
// Material-map cells
// ============================================================================

/// A cell holding `slot` alone.
#[inline]
pub fn material_cell(slot: u8) -> MaterialCell {
    [slot, MATERIAL_SLOT_NONE, 0, 0]
}

/// The cell for materials `a` and `b` with `b` weighted `blend_b / 255`, in
/// the form every writer stores: the stronger material in `id_a` (so
/// `blend_b <= 127`, ties going to `a`), and an unused `id_b` of
/// [`MATERIAL_SLOT_NONE`] with a zero blend. One form per mix keeps undo
/// diffs and matmap bytes stable, and lets a reader take `id_a` as the
/// dominant material without looking at the blend.
pub fn canonical_material_cell(a: u8, b: u8, blend_b: u8) -> MaterialCell {
    if a == MATERIAL_SLOT_NONE {
        return if b == MATERIAL_SLOT_NONE || blend_b == 0 {
            [MATERIAL_SLOT_NONE, MATERIAL_SLOT_NONE, 0, 0]
        } else {
            material_cell(b)
        };
    }
    if b == MATERIAL_SLOT_NONE || b == a || blend_b == 0 {
        return material_cell(a);
    }
    if blend_b == 255 {
        return material_cell(b);
    }
    if blend_b > 127 {
        [b, a, 255 - blend_b, 0]
    } else {
        [a, b, blend_b, 0]
    }
}

/// The `(slot, weight)` pairs `cell` holds, weights summing to 1 unless the
/// cell has no material. An unused entry is `(MATERIAL_SLOT_NONE, 0.0)`, and
/// a pair can carry a zero weight (`blend_b` of 0 or 255 on a hand-made
/// cell), so consumers skip both. A cell whose `id_a` is none but whose
/// `id_b` is not gives `id_b` the whole weight.
pub fn material_cell_weights(cell: MaterialCell) -> [(u8, f32); 2] {
    const UNUSED: (u8, f32) = (MATERIAL_SLOT_NONE, 0.0);
    let [a, b, blend, _] = cell;
    let a_none = a == MATERIAL_SLOT_NONE;
    let b_none = b == MATERIAL_SLOT_NONE || blend == 0 || b == a;
    match (a_none, b_none) {
        (true, true) => [UNUSED, UNUSED],
        (true, false) => [(b, 1.0), UNUSED],
        (false, true) => [(a, 1.0), UNUSED],
        (false, false) => {
            let wb = blend as f32 / 255.0;
            [(a, 1.0 - wb), (b, wb)]
        }
    }
}

/// `cell` after painting `slot` into it at `strength` (0 to 1).
///
/// The cell's weights are lerped toward all of `slot` by `strength`, and
/// then cut back to two materials:
/// `slot` always stays, with the strongest of the others beside it. Keeping
/// the painted slot is what makes a stroke converge. A rule keeping the top
/// two by weight would drop a new material entering a 50/50 mix at a
/// strength below one third, and repeated dabs would never get it in.
///
/// The blend is rounded toward `slot` (down when it is `id_b`'s share and
/// `slot` is `id_a`, up when `slot` is the weaker `id_b`), so every dab that
/// moves the weights at all moves the stored cell at least one 1/255 step,
/// and repeated dabs at any strength end at `slot` alone instead of stalling
/// on a rounding fixed point. Full strength replaces the cell outright. A
/// non-positive or NaN strength, or painting "no material", changes nothing.
pub fn paint_material_cell(cell: MaterialCell, slot: u8, strength: f32) -> MaterialCell {
    if slot == MATERIAL_SLOT_NONE || !(strength > 0.0) {
        return cell;
    }
    if strength >= 1.0 {
        return material_cell(slot);
    }
    let keep = 1.0 - strength;
    let mut painted = strength;
    let mut other: Option<(u8, f32)> = None;
    for (s, w) in material_cell_weights(cell) {
        if s == MATERIAL_SLOT_NONE || !(w > 0.0) {
            continue;
        }
        if s == slot {
            painted += w * keep;
        } else {
            let w = w * keep;
            // Strictly greater, so a tie keeps `id_a`, the cell's first entry.
            if other.map_or(true, |(_, best)| w > best) {
                other = Some((s, w));
            }
        }
    }
    let Some((other_slot, other_weight)) = other else {
        return material_cell(slot);
    };
    let total = painted + other_weight;
    if painted >= other_weight {
        let blend = (other_weight / total * 255.0).floor().clamp(0.0, 255.0) as u8;
        canonical_material_cell(slot, other_slot, blend)
    } else {
        let blend = (painted / total * 255.0).ceil().clamp(0.0, 255.0) as u8;
        canonical_material_cell(other_slot, slot, blend)
    }
}

/// Whether the legacy splatmap's fourth bucket means Snow (else Water) for a
/// cell at world height `world_height`: at or above
/// [`LEGACY_SNOW_ALTITUDE_FRACTION`] of the band, measured up from
/// `height_offset`. Multiplied through rather than divided, so a zero
/// `height_scale` needs no guard, exactly as the old mesher compared it.
#[inline]
pub fn legacy_bucket3_is_snow(config: &TerrainConfig, world_height: f32) -> bool {
    world_height - config.height_offset >= config.height_scale * LEGACY_SNOW_ALTITUDE_FRACTION
}

/// Convert one legacy splatmap pixel, channel bytes `[grass, rock, dirt,
/// snow-or-water]`, to a material cell: the two heaviest channels (ties to
/// the lower channel) become Grass, Rock, Dirt and, for the fourth, Snow
/// when `bucket3_is_snow` (see [`legacy_bucket3_is_snow`]) or Water, blended
/// by their relative weight. The third and fourth heaviest channels are
/// dropped, the same top-two cut the material map makes everywhere. A pixel
/// with no weight at all is Grass, the default a fresh material layer starts
/// from.
pub fn legacy_splat_to_material_cell(bytes: [u8; 4], bucket3_is_snow: bool) -> MaterialCell {
    let bucket_slot = |bucket: usize| -> u8 {
        let material = match bucket {
            0 => TerrainMaterial::Grass,
            1 => TerrainMaterial::Rock,
            2 => TerrainMaterial::Dirt,
            _ if bucket3_is_snow => TerrainMaterial::Snow,
            _ => TerrainMaterial::Water,
        };
        material.to_u8()
    };
    let mut order = [0usize, 1, 2, 3];
    order.sort_by(|&x, &y| bytes[y].cmp(&bytes[x]).then(x.cmp(&y)));
    let (a, b) = (order[0], order[1]);
    let (weight_a, weight_b) = (bytes[a] as u32, bytes[b] as u32);
    if weight_a == 0 {
        return material_cell(TerrainMaterial::Grass.to_u8());
    }
    if weight_b == 0 {
        return material_cell(bucket_slot(a));
    }
    // Floor, and `weight_b <= weight_a`, so the blend stays at or under 127.
    let blend = (weight_b * 255 / (weight_a + weight_b)) as u8;
    canonical_material_cell(bucket_slot(a), bucket_slot(b), blend)
}

/// Material slots and their weights at one point, strongest first, for CPU
/// consumers (vertex colouring, gameplay queries). Holds up to eight slots,
/// the most a bilinear read of four two-material cells can name, without
/// allocating. See `height_query::material_weights_at_world`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotWeights {
    entries: [(u8, f32); 8],
    len: usize,
}

impl Default for SlotWeights {
    fn default() -> Self {
        Self { entries: [(MATERIAL_SLOT_NONE, 0.0); 8], len: 0 }
    }
}

impl SlotWeights {
    /// Add `weight` to `slot`, merging with an entry already holding it.
    /// "No material", non-positive and NaN weights are ignored, and a ninth
    /// distinct slot (impossible from four cells) is dropped.
    pub fn add(&mut self, slot: u8, weight: f32) {
        if slot == MATERIAL_SLOT_NONE || !(weight > 0.0) {
            return;
        }
        if let Some(entry) = self.entries[..self.len].iter_mut().find(|(s, _)| *s == slot) {
            entry.1 += weight;
        } else if self.len < self.entries.len() {
            self.entries[self.len] = (slot, weight);
            self.len += 1;
        }
    }

    /// Scale the weights to sum to 1 and order them strongest first (ties:
    /// lower slot first, so the order is deterministic).
    pub fn normalize(&mut self) {
        let sum: f32 = self.entries[..self.len].iter().map(|(_, w)| w).sum();
        if !(sum > 0.0) {
            self.len = 0;
            return;
        }
        for entry in &mut self.entries[..self.len] {
            entry.1 /= sum;
        }
        self.entries[..self.len].sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    }

    /// The `(slot, weight)` entries, strongest first after [`Self::normalize`].
    pub fn as_slice(&self) -> &[(u8, f32)] {
        &self.entries[..self.len]
    }

    /// No slot carries any weight: the point has no material layer, or only
    /// cells without a material around it.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The strongest slot, `None` when [`Self::is_empty`].
    pub fn dominant(&self) -> Option<u8> {
        self.as_slice().first().map(|(slot, _)| *slot)
    }

    /// The weight of `slot`, 0 when it is absent.
    pub fn weight_of(&self, slot: u8) -> f32 {
        self.as_slice().iter().find(|(s, _)| *s == slot).map_or(0.0, |(_, w)| *w)
    }
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
    fn from_name_matches_every_variant_loosely() {
        for m in TerrainMaterial::all() {
            assert_eq!(TerrainMaterial::from_name(m.name()), Some(*m));
            assert_eq!(TerrainMaterial::from_name(&m.name().to_uppercase()), Some(*m));
        }
        assert_eq!(TerrainMaterial::from_name("wood planks"), Some(TerrainMaterial::WoodPlanks));
        assert_eq!(TerrainMaterial::from_name("cracked_lava"), Some(TerrainMaterial::CrackedLava));
        assert_eq!(TerrainMaterial::from_name("leafy-grass"), Some(TerrainMaterial::LeafyGrass));
        assert_eq!(TerrainMaterial::from_name("lava"), None);
        assert_eq!(TerrainMaterial::from_name(""), None);
    }

    /// Weight of `slot` in `cell`.
    fn weight(cell: MaterialCell, slot: u8) -> f32 {
        material_cell_weights(cell)
            .iter()
            .filter(|(s, _)| *s == slot)
            .map(|(_, w)| *w)
            .sum()
    }

    const GRASS: u8 = TerrainMaterial::Grass as u8;
    const ROCK: u8 = TerrainMaterial::Rock as u8;
    const SAND: u8 = TerrainMaterial::Sand as u8;

    #[test]
    fn full_strength_paint_replaces_the_cell() {
        assert_eq!(paint_material_cell(material_cell(GRASS), ROCK, 1.0), material_cell(ROCK));
        let mixed = canonical_material_cell(GRASS, SAND, 90);
        assert_eq!(paint_material_cell(mixed, ROCK, 1.0), material_cell(ROCK));
        assert_eq!(paint_material_cell(mixed, ROCK, 7.0), material_cell(ROCK), "strength clamps at 1");
        // A cell with no material takes any paint whole.
        assert_eq!(paint_material_cell([MATERIAL_SLOT_NONE; 4], SAND, 0.2), material_cell(SAND));
        // A custom slot paints like a built-in one.
        assert_eq!(paint_material_cell(material_cell(GRASS), 200, 1.0), material_cell(200));
    }

    #[test]
    fn partial_strength_paint_blends_and_repeated_dabs_converge() {
        let once = paint_material_cell(material_cell(GRASS), ROCK, 0.5);
        assert!((weight(once, ROCK) - 0.5).abs() <= 1.0 / 255.0 + 1e-6, "{once:?}");
        assert!((weight(once, GRASS) - 0.5).abs() <= 1.0 / 255.0 + 1e-6, "{once:?}");
        assert!(once[2] <= 127, "the stronger material is stored first: {once:?}");

        for strength in [0.05f32, 0.3, 0.7] {
            let mut cell = material_cell(GRASS);
            let mut previous = weight(cell, ROCK);
            let mut dabs = 0;
            while cell != material_cell(ROCK) {
                cell = paint_material_cell(cell, ROCK, strength);
                let now = weight(cell, ROCK);
                assert!(now > previous, "a dab at {strength} did not move {cell:?}");
                previous = now;
                dabs += 1;
                assert!(dabs < 400, "paint at {strength} never converged: {cell:?}");
            }
        }
    }

    #[test]
    fn painting_a_third_material_keeps_it_and_the_strongest_other() {
        // Grass 0.6, Rock 0.4.
        let mixed = canonical_material_cell(GRASS, ROCK, 102);
        let painted = paint_material_cell(mixed, SAND, 0.3);
        let slots: Vec<u8> = material_cell_weights(painted)
            .iter()
            .filter(|(s, w)| *s != MATERIAL_SLOT_NONE && *w > 0.0)
            .map(|(s, _)| *s)
            .collect();
        assert_eq!(slots, vec![GRASS, SAND], "Rock, the weakest after the dab, is dropped: {painted:?}");
        // Grass 0.42 against Sand 0.3, renormalized.
        assert!((weight(painted, SAND) - 0.3 / 0.72).abs() <= 1.0 / 255.0 + 1e-6);

        // Even a weak dab into an even mix gets the new material in, where
        // a plain top-two cut would drop it every time.
        let even = canonical_material_cell(GRASS, ROCK, 127);
        let weak = paint_material_cell(even, SAND, 0.1);
        assert!(weight(weak, SAND) > 0.0, "{weak:?}");
        let mut cell = weak;
        for _ in 0..400 {
            cell = paint_material_cell(cell, SAND, 0.1);
        }
        assert_eq!(cell, material_cell(SAND));
    }

    #[test]
    fn no_op_paints_leave_the_cell_alone() {
        let cell = canonical_material_cell(GRASS, ROCK, 40);
        assert_eq!(paint_material_cell(cell, SAND, 0.0), cell);
        assert_eq!(paint_material_cell(cell, SAND, -1.0), cell);
        assert_eq!(paint_material_cell(cell, SAND, f32::NAN), cell);
        assert_eq!(paint_material_cell(cell, MATERIAL_SLOT_NONE, 1.0), cell);
    }

    #[test]
    fn canonical_cells_store_the_stronger_material_first() {
        assert_eq!(canonical_material_cell(GRASS, ROCK, 200), [ROCK, GRASS, 55, 0]);
        assert_eq!(canonical_material_cell(GRASS, ROCK, 127), [GRASS, ROCK, 127, 0]);
        assert_eq!(canonical_material_cell(GRASS, ROCK, 0), material_cell(GRASS));
        assert_eq!(canonical_material_cell(GRASS, ROCK, 255), material_cell(ROCK));
        assert_eq!(canonical_material_cell(GRASS, GRASS, 90), material_cell(GRASS));
        assert_eq!(canonical_material_cell(MATERIAL_SLOT_NONE, ROCK, 10), material_cell(ROCK));
        assert_eq!(material_cell_weights(material_cell(SAND)), [(SAND, 1.0), (MATERIAL_SLOT_NONE, 0.0)]);
    }

    #[test]
    fn legacy_splat_pixels_convert_to_their_two_heaviest_buckets() {
        let snow = TerrainMaterial::Snow.to_u8();
        let water = TerrainMaterial::Water.to_u8();
        let dirt = TerrainMaterial::Dirt.to_u8();
        assert_eq!(legacy_splat_to_material_cell([255, 0, 0, 0], false), material_cell(GRASS));
        assert_eq!(legacy_splat_to_material_cell([0, 0, 0, 0], true), material_cell(GRASS), "blank is grass");
        assert_eq!(legacy_splat_to_material_cell([0, 0, 0, 255], true), material_cell(snow));
        assert_eq!(legacy_splat_to_material_cell([0, 0, 0, 255], false), material_cell(water));
        // Rock 153, dirt 102: rock first, dirt at 102 / 255 of the pair.
        assert_eq!(legacy_splat_to_material_cell([0, 153, 102, 0], false), [ROCK, dirt, 102, 0]);
        // Four channels: the lightest two go, the pair renormalizes.
        let cell = legacy_splat_to_material_cell([30, 100, 25, 100], false);
        assert_eq!(cell, [ROCK, water, 127, 0], "ties go to the lower channel first");
    }

    #[test]
    fn the_legacy_snow_line_is_a_fraction_of_the_band_above_its_floor() {
        let config = TerrainConfig { height_offset: -40.0, height_scale: 100.0, ..TerrainConfig::default() };
        // The snow line is 72 m above the -40 m floor: Y = 32.
        assert!(legacy_bucket3_is_snow(&config, 32.0));
        assert!(legacy_bucket3_is_snow(&config, 60.0));
        assert!(!legacy_bucket3_is_snow(&config, 31.9));
        assert!(!legacy_bucket3_is_snow(&config, -40.0));
    }

    #[test]
    fn slot_weights_merge_normalize_and_order() {
        let mut weights = SlotWeights::default();
        assert!(weights.is_empty());
        weights.add(ROCK, 0.25);
        weights.add(GRASS, 0.5);
        weights.add(ROCK, 0.5);
        weights.add(MATERIAL_SLOT_NONE, 3.0);
        weights.add(SAND, 0.0);
        weights.normalize();
        assert_eq!(weights.as_slice().len(), 2);
        assert_eq!(weights.dominant(), Some(ROCK));
        assert!((weights.weight_of(ROCK) - 0.6).abs() < 1e-6);
        assert!((weights.weight_of(GRASS) - 0.4).abs() < 1e-6);
        assert_eq!(weights.weight_of(SAND), 0.0);
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
