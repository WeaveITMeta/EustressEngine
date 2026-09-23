//! Terrain material slots: what each material-map slot id looks like and how
//! it behaves.
//!
//! A material-map cell names two of 256 slots (see the `material` module
//! docs). [`TerrainMaterialSlots`] says what those slots are: slots
//! `0..MATERIAL_COUNT` are the built-in [`TerrainMaterial`] variants, each
//! drawn with one of the bundled texture sets in
//! `common/assets/materials/textures/`; slots `FIRST_CUSTOM_MATERIAL_SLOT..=254`
//! are a Space's own materials; slot 255 is never defined.
//!
//! ## Where custom slots come from
//!
//! [`TerrainMaterialSlots::load_from_terrain_dir`] reads a Space's
//! `Workspace/Terrain`:
//!
//! 1. every `[[materials.palette]]` entry of `_terrain.toml`, whose `slot`
//!    names the slot and whose `file` is a `.mat.toml` relative to the
//!    terrain directory;
//! 2. every `materials/*.mat.toml` the palette does not list that carries a
//!    `slot` key of its own. A file with neither is not a terrain material.
//!
//! A custom slot starts as a copy of its `base` built-in (the `base` key, else
//! the built-in its name matches, else Grass) and every key the file sets
//! overrides it. An entry for a built-in slot overrides that built-in the same
//! way, but only with the keys it actually sets: the palette files earlier
//! exporters wrote carry empty texture paths and the template's legacy tiling,
//! and neither strips a built-in of its textures. The first definition of a
//! slot wins; a later one is reported and skipped.
//!
//! ## Colour, tint and the vertex-colour fallback
//!
//! `tint` is a linear RGB multiplier on the albedo texture. A built-in's tint
//! is its `TerrainMaterial::base_color` divided by the mean albedo of its
//! texture set ([`BUNDLED_TEXTURE_SETS`]), so the textured ground averages to
//! the colour the material has always had. A tint belongs with its texture: a
//! slot that brings its own texture set starts from a white tint.
//!
//! [`MaterialSlot::swatch_linear`] is the colour a slot reads as from a
//! distance, the texture mean times the tint. It colours the picker, and the
//! vertex-colour mesher paints it through [`TerrainSlotPalette`] (kept on
//! `TerrainData`) for as long as the textured terrain material is not up.
//!
//! ## Reloading
//!
//! [`TerrainMaterialSource`] names the terrain directory. The host points it
//! at the active Space; [`reload_terrain_material_slots`] rebuilds the table
//! whenever it changes or a reload is requested, and
//! [`watch_terrain_material_files`] requests one when a `FileChanged` message
//! reports `_terrain.toml` or a `materials/*.mat.toml` changing. Hosts without
//! a file watcher (the Client) simply never send those messages.
//!
//! ## Adding a material, physics and gameplay
//!
//! The Studio's "Add material" action takes
//! [`TerrainMaterialSlots::next_unused_custom_slot`], writes the file with
//! [`write_custom_material_toml`] and calls
//! [`TerrainMaterialSource::request_reload`]. A slot's `physics_material`
//! gives terrain colliders their friction ([`TerrainMaterialSlots::friction`],
//! applied by `collider::apply_terrain_chunk_friction`), and
//! [`TerrainMaterialQuery`] answers what the ground at a point is made of.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
// Explicit, so the log macros do not depend on the prelude's `bevy_log`
// feature (see `avatar::boot`).
use tracing::{debug, info, warn};

use super::height_query::{material_at_world, MaterialSample};
use super::material::{
    MaterialCell, TerrainMaterial, FIRST_CUSTOM_MATERIAL_SLOT, MATERIAL_SLOT_COUNT, MATERIAL_SLOT_NONE,
};
use super::texture_arrays::TerrainTextureArrays;
use super::toml_loader::{self, MaterialTiling, MaterialTomlDef};
use super::{surface_data, TerrainBaked, TerrainBrush, TerrainConfig, TerrainData, TerrainDirtyChunks, TerrainRoot};
use crate::file_events::FileChanged;

/// Directory, relative to the bundled asset root, holding the bundled
/// texture sets.
pub const BUNDLED_TEXTURE_DIR: &str = "materials/textures";

/// The texture sets shipped in [`BUNDLED_TEXTURE_DIR`], with the mean linear
/// albedo of each `{name}_base_color.png`.
///
/// The means were measured from the shipped 2048x2048 maps (every texel
/// decoded to linear light and averaged). They let the slot table give
/// built-ins their calibrated tints and every slot its swatch without
/// decoding a texture; the texture arrays measure their own layer means
/// again when they build (`TerrainTextureLayer`). Re-measure them when a
/// set is regenerated.
pub const BUNDLED_TEXTURE_SETS: [(&str, [f32; 3]); 18] = [
    ("brick", [0.3106, 0.1057, 0.0707]),
    ("bronze", [0.6649, 0.3024, 0.1064]),
    ("concrete", [0.3036, 0.2948, 0.2693]),
    ("corroded_metal", [0.1990, 0.1138, 0.0895]),
    ("diamond_plate", [0.4954, 0.5131, 0.5433]),
    ("fabric", [0.3795, 0.3330, 0.2616]),
    ("foil", [0.7530, 0.7681, 0.7912]),
    ("gold", [0.9951, 0.7573, 0.3122]),
    ("granite", [0.4745, 0.4419, 0.4168]),
    ("grass", [0.0656, 0.1278, 0.0172]),
    ("ice", [0.6207, 0.7347, 0.8305]),
    ("marble", [0.8054, 0.7994, 0.7768]),
    ("metal", [0.5274, 0.5398, 0.5586]),
    ("sand", [0.6139, 0.4656, 0.2618]),
    ("silver", [0.8958, 0.8782, 0.8280]),
    ("slate", [0.0531, 0.0604, 0.0698]),
    ("wood", [0.4332, 0.2322, 0.0946]),
    ("wood_planks", [0.2776, 0.1319, 0.0495]),
];

/// sRGB swatch of a slot no definition covers: a neutral grey, so painted
/// cells whose material file went missing stay visible.
pub const UNDEFINED_SLOT_SRGB: [f32; 3] = [0.5, 0.5, 0.5];

/// Largest tint component accepted. A calibrated built-in tint stays under 3
/// (Grass lifts the grass set's deep blue channel the most); anything far
/// past that is a typo that would blow the albedo out.
pub const MAX_TINT: f32 = 8.0;

/// Tiling limits, in metres per texture repeat.
const MIN_TILING: f32 = 0.05;
const MAX_TILING: f32 = 10_000.0;

/// The bundled set called `name`, ignoring ASCII case.
pub fn bundled_texture_set(name: &str) -> Option<&'static str> {
    BUNDLED_TEXTURE_SETS
        .iter()
        .map(|(set, _)| *set)
        .find(|set| set.eq_ignore_ascii_case(name.trim()))
}

/// Measured mean linear albedo of bundled set `name` (see
/// [`BUNDLED_TEXTURE_SETS`]).
pub fn bundled_texture_set_mean(name: &str) -> Option<[f32; 3]> {
    BUNDLED_TEXTURE_SETS
        .iter()
        .find(|(set, _)| set.eq_ignore_ascii_case(name))
        .map(|(_, mean)| *mean)
}

/// The images one material slot is drawn with. Slots naming equal sets share
/// one texture-array layer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TerrainTextureSet {
    /// A bundled set: `{name}_base_color.png`, `{name}_normal.png` and
    /// `{name}_metallic_roughness.png` (occlusion, roughness and metallic in
    /// R, G and B) in [`BUNDLED_TEXTURE_DIR`].
    Bundled(&'static str),
    /// A Space's own images, absolute paths. A missing normal map reads as
    /// flat, a missing ORM map as full occlusion, roughness 1 and metallic 0.
    Files {
        albedo: PathBuf,
        normal: Option<PathBuf>,
        orm: Option<PathBuf>,
    },
}

/// The files a [`TerrainTextureSet`] reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureSetFiles {
    pub albedo: PathBuf,
    pub normal: Option<PathBuf>,
    pub orm: Option<PathBuf>,
}

impl TerrainTextureSet {
    /// The files this set reads, bundled sets resolved under `bundled_root`
    /// (`avatar::boot::bundled_root()` in a running app).
    pub fn files(&self, bundled_root: &Path) -> TextureSetFiles {
        match self {
            Self::Bundled(name) => {
                let dir = bundled_root.join(BUNDLED_TEXTURE_DIR);
                TextureSetFiles {
                    albedo: dir.join(format!("{name}_base_color.png")),
                    normal: Some(dir.join(format!("{name}_normal.png"))),
                    orm: Some(dir.join(format!("{name}_metallic_roughness.png"))),
                }
            }
            Self::Files { albedo, normal, orm } => TextureSetFiles {
                albedo: albedo.clone(),
                normal: normal.clone(),
                orm: orm.clone(),
            },
        }
    }

    /// Short name for logs: the bundled set's name, else the albedo path.
    pub fn label(&self) -> String {
        match self {
            Self::Bundled(name) => (*name).to_string(),
            Self::Files { albedo, .. } => albedo.display().to_string(),
        }
    }

    /// Whether `path` is one of this set's own files. Bundled sets never
    /// match: they are not part of a Space.
    pub fn uses_file(&self, path: &Path) -> bool {
        match self {
            Self::Bundled(_) => false,
            Self::Files { albedo, normal, orm } => {
                albedo == path || normal.as_deref() == Some(path) || orm.as_deref() == Some(path)
            }
        }
    }
}

/// What one material slot looks like and how it behaves.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialSlot {
    /// Display name.
    pub name: String,
    /// The built-in material this slot is, or starts from.
    pub base: TerrainMaterial,
    /// The images the slot is drawn with; `None` draws it flat in its tint.
    pub texture_set: Option<TerrainTextureSet>,
    /// Linear RGB multiplier on the albedo texture, or the flat colour of a
    /// slot without a texture set.
    pub tint: [f32; 3],
    /// Metres of ground one texture repeat covers.
    pub tiling: f32,
    /// Perceptual roughness the slot averages to (0 mirror, 1 matte). The
    /// renderer scales the texture's roughness channel so its layer mean
    /// lands here (see `TerrainSlotSurface`).
    pub roughness: f32,
    /// Metallic factor, applied uniformly.
    pub metallic: f32,
    /// Name the realism material registry resolves friction from, `None`
    /// where it has no fitting entry (the collider keeps its default).
    pub physics_material: Option<String>,
    /// The `.mat.toml` that defined or overrode this slot, `None` for a
    /// built-in as shipped.
    pub source: Option<PathBuf>,
}

impl MaterialSlot {
    /// Linear RGB this slot reads as from a distance: its texture set's mean
    /// albedo times its tint. A Space's own texture files have no measured
    /// mean before they are decoded, so they read as the base material's
    /// colour times the tint; a slot without a texture set is its tint.
    pub fn swatch_linear(&self) -> [f32; 3] {
        let under = match &self.texture_set {
            Some(TerrainTextureSet::Bundled(name)) => {
                bundled_texture_set_mean(name).unwrap_or_else(|| linear_rgb(self.base.base_color()))
            }
            Some(TerrainTextureSet::Files { .. }) => linear_rgb(self.base.base_color()),
            None => [1.0; 3],
        };
        std::array::from_fn(|i| (under[i] * self.tint[i]).clamp(0.0, 1.0))
    }

    /// [`Self::swatch_linear`] encoded as sRGB, for the mesher and the UI.
    pub fn swatch_srgb(&self) -> [f32; 3] {
        let [r, g, b] = self.swatch_linear();
        let srgb = Color::linear_rgb(r, g, b).to_srgba();
        [srgb.red, srgb.green, srgb.blue]
    }
}

/// Linear RGB of `color`.
fn linear_rgb(color: Color) -> [f32; 3] {
    let linear = color.to_linear();
    [linear.red, linear.green, linear.blue]
}

/// How a built-in material is drawn: its bundled texture set, metres per
/// texture repeat, and the realism material its friction comes from.
struct BuiltinLook {
    set: &'static str,
    tiling: f32,
    physics: Option<&'static str>,
}

/// The built-in materials' texture sets. Materials without a set of their
/// own borrow the closest one and get their colour from the calibrated tint
/// (Mud, Dirt and Ground are tinted sand; Water and Snow tinted ice;
/// Asphalt tinted concrete; CrackedLava tinted slate). Physics names are ones
/// `realism::materials::properties::MaterialProperties::from_name` resolves,
/// and `None` where it has no fitting entry: soil, snow and salt would all
/// land on a hard-solid preset.
fn builtin_look(material: TerrainMaterial) -> BuiltinLook {
    use TerrainMaterial::*;
    let (set, tiling, physics) = match material {
        Grass => ("grass", 4.0, Some("Grass")),
        LeafyGrass => ("grass", 3.0, Some("Grass")),
        Rock => ("granite", 8.0, Some("Granite")),
        Cobblestone => ("granite", 3.0, Some("Cobblestone")),
        Slate => ("slate", 6.0, Some("Slate")),
        Basalt => ("slate", 6.0, Some("Granite")),
        CrackedLava => ("slate", 6.0, None),
        Sand => ("sand", 4.0, Some("Sand")),
        Sandstone => ("sand", 6.0, None),
        Dirt => ("sand", 4.0, None),
        Ground => ("sand", 4.0, None),
        Mud => ("sand", 3.0, None),
        Limestone => ("marble", 8.0, Some("Marble")),
        Salt => ("marble", 6.0, None),
        Snow => ("ice", 6.0, None),
        Glacier => ("ice", 10.0, Some("Ice")),
        Ice => ("ice", 8.0, Some("Ice")),
        Water => ("ice", 8.0, None),
        Concrete => ("concrete", 4.0, Some("Concrete")),
        Pavement => ("concrete", 4.0, Some("Concrete")),
        Asphalt => ("concrete", 4.0, Some("Concrete")),
        Brick => ("brick", 3.0, Some("Brick")),
        WoodPlanks => ("wood_planks", 3.0, Some("WoodPlanks")),
    };
    BuiltinLook { set, tiling, physics }
}

/// The tint that makes bundled set `set` average to `material`'s base
/// colour.
fn calibrated_tint(material: TerrainMaterial, set: &str) -> [f32; 3] {
    let target = linear_rgb(material.base_color());
    let mean = bundled_texture_set_mean(set).unwrap_or([0.5; 3]);
    std::array::from_fn(|i| (target[i] / mean[i].max(1e-4)).clamp(0.0, MAX_TINT))
}

/// Built-in slot `material` as shipped.
pub fn builtin_slot(material: TerrainMaterial) -> MaterialSlot {
    let look = builtin_look(material);
    MaterialSlot {
        name: material.name().to_string(),
        base: material,
        texture_set: Some(TerrainTextureSet::Bundled(look.set)),
        tint: calibrated_tint(material, look.set),
        tiling: look.tiling,
        roughness: material.roughness(),
        metallic: material.metallic(),
        physics_material: look.physics.map(str::to_string),
        source: None,
    }
}

/// sRGB swatch of every slot, shared by reference. `TerrainData` carries
/// one, so the vertex-colour mesher, which only sees the terrain's config
/// and data, colours custom slots from the Space's slot table.
#[derive(Clone)]
pub struct TerrainSlotPalette(Arc<[[f32; 3]; MATERIAL_SLOT_COUNT]>);

impl TerrainSlotPalette {
    /// Swatches of `slots`, undefined slots in [`UNDEFINED_SLOT_SRGB`].
    fn from_slots(slots: &[Option<MaterialSlot>]) -> Self {
        let mut colors = [UNDEFINED_SLOT_SRGB; MATERIAL_SLOT_COUNT];
        for (color, slot) in colors.iter_mut().zip(slots) {
            if let Some(slot) = slot {
                *color = slot.swatch_srgb();
            }
        }
        Self(Arc::new(colors))
    }

    /// sRGB swatch of `slot`.
    #[inline]
    pub fn srgb(&self, slot: u8) -> [f32; 3] {
        self.0[slot as usize]
    }

    /// Swatch of `slot` as a `Color`.
    pub fn color(&self, slot: u8) -> Color {
        let [r, g, b] = self.srgb(slot);
        Color::srgb(r, g, b)
    }
}

impl Default for TerrainSlotPalette {
    /// The built-in slots as shipped, built once and shared.
    fn default() -> Self {
        static BUILTIN: OnceLock<TerrainSlotPalette> = OnceLock::new();
        BUILTIN
            .get_or_init(|| TerrainSlotPalette::from_slots(&builtin_slot_list()))
            .clone()
    }
}

impl PartialEq for TerrainSlotPalette {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || *self.0 == *other.0
    }
}

impl std::fmt::Debug for TerrainSlotPalette {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TerrainSlotPalette(..)")
    }
}

/// Slots 0..=255 with only the built-ins defined.
fn builtin_slot_list() -> Vec<Option<MaterialSlot>> {
    (0..MATERIAL_SLOT_COUNT)
        .map(|slot| u8::try_from(slot).ok().and_then(TerrainMaterial::from_u8).map(builtin_slot))
        .collect()
}

/// The active terrain's material slot table. See the module docs.
#[derive(Resource, Clone, Debug)]
pub struct TerrainMaterialSlots {
    /// One entry per slot id, `MATERIAL_SLOT_COUNT` long.
    slots: Vec<Option<MaterialSlot>>,
    /// Swatches of `slots`, rebuilt whenever they change.
    palette: TerrainSlotPalette,
}

impl PartialEq for TerrainMaterialSlots {
    fn eq(&self, other: &Self) -> bool {
        // The palette is derived from the slots.
        self.slots == other.slots
    }
}

impl Default for TerrainMaterialSlots {
    fn default() -> Self {
        Self::builtins()
    }
}

impl TerrainMaterialSlots {
    /// Only the 23 built-in slots, as shipped.
    pub fn builtins() -> Self {
        Self { slots: builtin_slot_list(), palette: TerrainSlotPalette::default() }
    }

    /// Slot `slot`'s definition, `None` when nothing defines it.
    pub fn get(&self, slot: u8) -> Option<&MaterialSlot> {
        self.slots.get(slot as usize).and_then(Option::as_ref)
    }

    /// Every defined slot, in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (u8, &MaterialSlot)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(slot, def)| Some((u8::try_from(slot).ok()?, def.as_ref()?)))
    }

    /// Define, redefine or clear `slot`. Clearing a built-in slot restores it
    /// as shipped (built-ins are always defined), and slot 255 cannot be
    /// defined at all.
    pub fn set_slot(&mut self, slot: u8, definition: Option<MaterialSlot>) {
        if slot == MATERIAL_SLOT_NONE {
            return;
        }
        let definition = definition.or_else(|| TerrainMaterial::from_u8(slot).map(builtin_slot));
        self.slots[slot as usize] = definition;
        self.rebuild_palette();
    }

    /// Swatches of every slot, the palette `TerrainData` carries.
    pub fn palette(&self) -> &TerrainSlotPalette {
        &self.palette
    }

    /// sRGB swatch of `slot` ([`UNDEFINED_SLOT_SRGB`] when undefined).
    pub fn swatch_srgb(&self, slot: u8) -> [f32; 3] {
        self.palette.srgb(slot)
    }

    /// The lowest custom slot that nothing defines and no cell of `cells` (a
    /// terrain's material map) names. A new material given a slot whose file
    /// was deleted would take over every cell still painted with it, so the
    /// "Add material" action picks its slot here.
    pub fn next_unused_custom_slot(&self, cells: &[MaterialCell]) -> Option<u8> {
        let mut named = [false; MATERIAL_SLOT_COUNT];
        for &[a, b, _, _] in cells {
            named[a as usize] = true;
            named[b as usize] = true;
        }
        (FIRST_CUSTOM_MATERIAL_SLOT..MATERIAL_SLOT_NONE)
            .find(|&slot| self.get(slot).is_none() && !named[slot as usize])
    }

    /// Static and kinetic friction coefficients of `slot`, from the realism
    /// material its `physics_material` names. `None` for an undefined slot, a
    /// slot without a physics material, or a name the registry does not
    /// resolve: the terrain collider then keeps Avian's default friction
    /// rather than guessing one.
    pub fn friction(&self, slot: u8) -> Option<(f32, f32)> {
        use crate::realism::materials::properties::MaterialProperties;
        let name = self.get(slot)?.physics_material.as_deref()?;
        let properties = MaterialProperties::from_name(name)?;
        let (static_coefficient, kinetic_coefficient) = (properties.friction_static, properties.friction_kinetic);
        (static_coefficient.is_finite()
            && kinetic_coefficient.is_finite()
            && static_coefficient >= 0.0
            && kinetic_coefficient >= 0.0)
            .then_some((static_coefficient, kinetic_coefficient))
    }

    /// Every distinct texture set the slots use, in the order of the first
    /// slot using each: the layers of the terrain texture arrays.
    pub fn unique_texture_sets(&self) -> Vec<TerrainTextureSet> {
        let mut sets: Vec<TerrainTextureSet> = Vec::new();
        for (_, slot) in self.iter() {
            if let Some(set) = &slot.texture_set {
                if !sets.contains(set) {
                    sets.push(set.clone());
                }
            }
        }
        sets
    }

    /// Any slot's texture set reads `path`.
    pub fn uses_texture_file(&self, path: &Path) -> bool {
        self.iter()
            .any(|(_, slot)| slot.texture_set.as_ref().is_some_and(|set| set.uses_file(path)))
    }

    fn rebuild_palette(&mut self) {
        self.palette = TerrainSlotPalette::from_slots(&self.slots);
    }

    /// The table for the terrain in `terrain_dir` (a Space's
    /// `Workspace/Terrain`): the built-ins, overridden and extended by its
    /// palette and material files (see the module docs), plus a message for
    /// everything that was skipped or corrected. A directory without a
    /// `_terrain.toml` or `materials/` just yields the built-ins.
    pub fn load_from_terrain_dir(terrain_dir: &Path) -> (Self, Vec<String>) {
        let mut slots = builtin_slot_list();
        let mut warnings = Vec::new();
        let mut defined_by: Vec<Option<PathBuf>> = vec![None; MATERIAL_SLOT_COUNT];
        let mut listed: Vec<PathBuf> = Vec::new();

        let toml_path = terrain_dir.join("_terrain.toml");
        if toml_path.is_file() {
            match toml_loader::load_terrain_toml(&toml_path) {
                Ok(file) => {
                    for entry in &file.materials.palette {
                        let path = terrain_dir.join(&entry.file);
                        // Listed even when it fails below, so the directory
                        // scan does not read it a second time.
                        listed.push(path.clone());
                        let slot = entry.slot;
                        if slot == MATERIAL_SLOT_NONE {
                            warnings.push(format!("{}: palette slot 255 is reserved for \"no material\"", toml_path.display()));
                            continue;
                        }
                        if let Some(first) = &defined_by[slot as usize] {
                            warnings.push(format!(
                                "{}: slot {slot} ({}) is already defined by {}; skipped",
                                toml_path.display(),
                                entry.name,
                                first.display()
                            ));
                            continue;
                        }
                        // The default template names files it never writes,
                        // so a missing file is the ordinary case, not an error.
                        let def = if path.is_file() {
                            match toml_loader::load_material_toml(&path) {
                                Ok(def) => Some(def),
                                Err(error) => {
                                    warnings.push(error);
                                    None
                                }
                            }
                        } else {
                            debug!("terrain material slot {slot}: {} does not exist", path.display());
                            None
                        };
                        if let Some(own) = def.as_ref().and_then(|def| def.slot) {
                            if own != slot {
                                warnings.push(format!(
                                    "{}: says slot {own}, the palette lists it as slot {slot}; the palette wins",
                                    path.display()
                                ));
                            }
                        }
                        let dir = path.parent().unwrap_or(terrain_dir).to_path_buf();
                        let source = def.is_some().then(|| path.clone());
                        slots[slot as usize] = Some(resolve_slot(
                            slot,
                            Some(entry.name.as_str()),
                            def.as_ref(),
                            &dir,
                            source,
                            &mut warnings,
                        ));
                        defined_by[slot as usize] = Some(path);
                    }
                }
                Err(error) => warnings.push(error),
            }
        }

        let materials_dir = terrain_dir.join("materials");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&materials_dir)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                    .filter(|path| {
                        path.is_file()
                            && path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with(".mat.toml"))
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Sorted, so which of two files claiming one slot wins does not
        // depend on the order the filesystem lists them in.
        files.sort();
        for path in files {
            if listed.iter().any(|known| known == &path) {
                continue;
            }
            let def = match toml_loader::load_material_toml(&path) {
                Ok(def) => def,
                Err(error) => {
                    warnings.push(error);
                    continue;
                }
            };
            let Some(slot) = def.slot else {
                debug!("{} has no slot key; not a terrain material slot", path.display());
                continue;
            };
            if slot == MATERIAL_SLOT_NONE {
                warnings.push(format!("{}: slot 255 is reserved for \"no material\"", path.display()));
                continue;
            }
            if let Some(first) = &defined_by[slot as usize] {
                warnings.push(format!(
                    "{}: slot {slot} is already defined by {}; skipped",
                    path.display(),
                    first.display()
                ));
                continue;
            }
            slots[slot as usize] =
                Some(resolve_slot(slot, None, Some(&def), &materials_dir, Some(path.clone()), &mut warnings));
            defined_by[slot as usize] = Some(path);
        }

        let mut table = Self { slots, palette: TerrainSlotPalette::default() };
        table.rebuild_palette();
        (table, warnings)
    }
}

/// Slot `slot` as `def` (read from a file in `def_dir`) defines it; see the
/// module docs for how its keys override the built-in it starts from.
fn resolve_slot(
    slot: u8,
    palette_name: Option<&str>,
    def: Option<&MaterialTomlDef>,
    def_dir: &Path,
    source: Option<PathBuf>,
    warnings: &mut Vec<String>,
) -> MaterialSlot {
    let non_empty = |text: &str| -> Option<String> {
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_string())
    };
    let def_name = def.and_then(|def| non_empty(def.name.as_str()));
    let listed_name = palette_name.and_then(non_empty);
    let base_key = def.and_then(|def| def.base.as_deref()).and_then(non_empty);

    let base = match TerrainMaterial::from_u8(slot) {
        Some(builtin) => {
            if let Some(key) = &base_key {
                if TerrainMaterial::from_name(key) != Some(builtin) {
                    warnings.push(format!(
                        "slot {slot} is the built-in {}; its base {key:?} is ignored",
                        builtin.name()
                    ));
                }
            }
            builtin
        }
        None => match &base_key {
            Some(key) => TerrainMaterial::from_name(key).unwrap_or_else(|| {
                warnings.push(format!("slot {slot}: unknown base material {key:?}, using Grass"));
                TerrainMaterial::Grass
            }),
            None => def_name
                .as_deref()
                .and_then(TerrainMaterial::from_name)
                .or_else(|| listed_name.as_deref().and_then(TerrainMaterial::from_name))
                .unwrap_or(TerrainMaterial::Grass),
        },
    };

    let mut resolved = builtin_slot(base);
    resolved.name = def_name
        .or(listed_name)
        .unwrap_or_else(|| match TerrainMaterial::from_u8(slot) {
            Some(builtin) => builtin.name().to_string(),
            None => format!("Material {slot}"),
        });
    resolved.source = source;

    let Some(def) = def else {
        return resolved;
    };

    // Textures: the file's own albedo, else a named bundled set, else the
    // base's set. A new set drops the base's tint, which was calibrated for
    // the base's texture, not this one.
    let file = |path: &str| non_empty(path).map(|path| def_dir.join(path));
    let set_key = def.texture_set.as_deref().and_then(non_empty);
    if let Some(albedo) = file(def.albedo.as_str()) {
        resolved.texture_set = Some(TerrainTextureSet::Files {
            albedo,
            normal: file(def.normal.as_str()),
            orm: file(def.orm.as_str()),
        });
        resolved.tint = [1.0; 3];
    } else if let Some(name) = set_key {
        if name.eq_ignore_ascii_case("none") {
            // A flat slot is its tint, so it starts at the base's colour.
            resolved.texture_set = None;
            resolved.tint = linear_rgb(base.base_color());
        } else if let Some(bundled) = bundled_texture_set(&name) {
            resolved.texture_set = Some(TerrainTextureSet::Bundled(bundled));
            resolved.tint = [1.0; 3];
        } else {
            warnings.push(format!("slot {slot}: unknown texture set {name:?}, keeping {}'s", base.name()));
        }
    }

    if let Some(tint) = def.tint {
        if tint.iter().all(|c| c.is_finite() && *c >= 0.0) {
            resolved.tint = tint.map(|c| c.min(MAX_TINT));
        } else {
            warnings.push(format!("slot {slot}: tint {tint:?} must be finite and not negative; ignored"));
        }
    }
    match def.tiling {
        Some(MaterialTiling::Metres(metres)) if metres.is_finite() && metres > 0.0 => {
            resolved.tiling = metres.clamp(MIN_TILING, MAX_TILING);
        }
        Some(MaterialTiling::Metres(metres)) => {
            warnings.push(format!("slot {slot}: tiling {metres} must be a positive number of metres; ignored"));
        }
        // Legacy per-chunk repeats: see `MaterialTiling`.
        Some(MaterialTiling::PerChunk(_)) | None => {}
    }
    if let Some(roughness) = def.roughness.filter(|r| r.is_finite()) {
        resolved.roughness = roughness.clamp(0.0, 1.0);
    }
    if let Some(metallic) = def.metallic.filter(|m| m.is_finite()) {
        resolved.metallic = metallic.clamp(0.0, 1.0);
    }
    if let Some(physics) = &def.physics_material {
        // An empty name clears the base's, for a slot that must keep the
        // collider's default friction.
        resolved.physics_material = non_empty(physics.as_str());
    }
    resolved
}

/// The `.mat.toml` text [`write_custom_material_toml`] writes: a custom slot
/// starting from `base`, with every optional key listed, commented out.
pub fn render_custom_material_toml(slot: u8, name: &str, base: TerrainMaterial) -> String {
    // A TOML basic string, so a quote or backslash in the name cannot break
    // the file.
    let name = toml::Value::String(name.trim().to_string()).to_string();
    format!(
        r#"# Terrain material slot {slot}.
# Starts as the built-in `base` material; every key set below overrides it.

[material]
name = {name}
slot = {slot}
base = "{base}"
# tint = [1.0, 1.0, 1.0]        # linear RGB multiplier on the albedo
# tiling = 4.0                  # metres of ground per texture repeat
# roughness = 0.85
# metallic = 0.0
# physics_material = "Granite"  # realism material the friction comes from
# texture_set = "granite"       # a bundled set, "none" for a flat tint, or your own maps:
# albedo = "textures/albedo.png"
# normal = "textures/normal.png"
# orm = "textures/orm.png"      # occlusion, roughness, metallic in R, G, B
"#,
        base = base.name(),
    )
}

/// Write `materials/<name>.mat.toml` under `terrain_dir`, defining custom
/// `slot` as a material starting from `base`, and return its path. Never
/// overwrites: a taken file name gets a numeric suffix. Refuses built-in
/// slots and slot 255. The table picks the file up on its next reload (see
/// [`TerrainMaterialSource::request_reload`]).
pub fn write_custom_material_toml(
    terrain_dir: &Path,
    slot: u8,
    name: &str,
    base: TerrainMaterial,
) -> Result<PathBuf, String> {
    if slot < FIRST_CUSTOM_MATERIAL_SLOT || slot == MATERIAL_SLOT_NONE {
        return Err(format!(
            "slot {slot} is not a custom material slot ({FIRST_CUSTOM_MATERIAL_SLOT}..=254)"
        ));
    }
    if name.trim().is_empty() {
        return Err("a terrain material needs a name".to_string());
    }
    let dir = terrain_dir.join("materials");
    std::fs::create_dir_all(&dir).map_err(|error| format!("Failed to create {}: {error}", dir.display()))?;

    let mut stem: String = name
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    stem = stem.split('_').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("_");
    if stem.is_empty() {
        stem = format!("material_{slot}");
    }
    // A file the palette names belongs to that palette slot even while it is
    // missing: the loader reads it for the palette's slot and the directory
    // scan skips it, so the new custom slot would never be defined.
    let palette_files: Vec<PathBuf> = toml_loader::load_terrain_toml(&terrain_dir.join("_terrain.toml"))
        .map(|file| file.materials.palette.iter().map(|entry| terrain_dir.join(&entry.file)).collect())
        .unwrap_or_default();
    let taken = |path: &Path| path.exists() || palette_files.iter().any(|listed| listed == path);
    let mut path = dir.join(format!("{stem}.mat.toml"));
    let mut suffix = 2;
    while taken(&path) {
        path = dir.join(format!("{stem}_{suffix}.mat.toml"));
        suffix += 1;
    }
    std::fs::write(&path, render_custom_material_toml(slot, name, base))
        .map_err(|error| format!("Failed to write {}: {error}", path.display()))?;
    Ok(path)
}

/// Where the active terrain's custom material slots come from. The host
/// points it at the open Space's `Workspace/Terrain`; `None` (the default,
/// and the Client's) means the built-ins only.
#[derive(Resource, Debug, Default)]
pub struct TerrainMaterialSource {
    terrain_dir: Option<PathBuf>,
    /// Bumped by [`Self::request_reload`], so a reload marks the resource
    /// changed even when the directory stays the same. Never read: the write
    /// through `ResMut` is the whole point.
    #[allow(dead_code)]
    reloads: u64,
}

impl TerrainMaterialSource {
    /// The terrain directory custom slots load from.
    pub fn terrain_dir(&self) -> Option<&Path> {
        self.terrain_dir.as_deref()
    }

    /// Load custom slots from `dir` from now on. Callers compare first
    /// (through `Deref`), since taking the resource mutably reloads.
    pub fn set_terrain_dir(&mut self, dir: Option<PathBuf>) {
        self.terrain_dir = dir;
    }

    /// Re-read the slot table from disk, after writing a material file.
    pub fn request_reload(&mut self) {
        self.reloads = self.reloads.wrapping_add(1);
    }
}

/// Rebuild [`TerrainMaterialSlots`] when [`TerrainMaterialSource`] changes.
/// The files are a `_terrain.toml` and a handful of small `.mat.toml`s, so
/// this reads them on the spot. An unchanged table is left alone, so nothing
/// downstream (palette, remesh, texture arrays) reacts to a no-op reload.
pub fn reload_terrain_material_slots(
    source: Res<TerrainMaterialSource>,
    mut slots: ResMut<TerrainMaterialSlots>,
) {
    if !source.is_changed() {
        return;
    }
    let (table, warnings) = match source.terrain_dir() {
        Some(dir) => TerrainMaterialSlots::load_from_terrain_dir(dir),
        None => (TerrainMaterialSlots::builtins(), Vec::new()),
    };
    for warning in &warnings {
        warn!(target: "eustress::terrain::materials", "{warning}");
    }
    if *slots != table {
        let custom = table.iter().filter(|(slot, _)| *slot >= FIRST_CUSTOM_MATERIAL_SLOT).count();
        let overridden = table
            .iter()
            .filter(|(slot, def)| *slot < FIRST_CUSTOM_MATERIAL_SLOT && def.source.is_some())
            .count();
        info!(
            target: "eustress::terrain::materials",
            custom,
            overridden,
            dir = ?source.terrain_dir(),
            "terrain material slots reloaded"
        );
        *slots = table;
    }
}

/// Request a slot reload when `_terrain.toml` or a `materials/*.mat.toml`
/// of the source directory changes on disk, and a texture-array rebuild
/// when an image a slot reads changes (its path, and so the array key, stays
/// the same). Reads the engine watcher's `FileChanged` broadcast; absent in
/// hosts without one.
pub fn watch_terrain_material_files(
    changes: Option<MessageReader<FileChanged>>,
    mut source: ResMut<TerrainMaterialSource>,
    slots: Res<TerrainMaterialSlots>,
    mut arrays: ResMut<TerrainTextureArrays>,
) {
    let Some(mut changes) = changes else {
        return;
    };
    if changes.is_empty() {
        return;
    }
    let (mut reload, mut retexture) = (false, false);
    {
        let terrain_dir = source.terrain_dir();
        // Read every message even without a directory, so none is left to
        // fire once one is set.
        for change in changes.read() {
            let Some(dir) = terrain_dir else { continue };
            let path = change.path.as_path();
            if !path.starts_with(dir) {
                continue;
            }
            let is_mat_toml =
                path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with(".mat.toml"));
            if path == dir.join("_terrain.toml")
                || (is_mat_toml && path.parent() == Some(dir.join("materials").as_path()))
            {
                reload = true;
            } else if slots.uses_texture_file(path) {
                retexture = true;
            }
        }
    }
    if reload {
        source.request_reload();
    }
    if retexture {
        arrays.request_rebuild();
    }
}

/// Keep every terrain root's `TerrainData::slot_palette` equal to the slot
/// table's, and remesh the chunks of a root whose palette changed in a way
/// its vertex colours show (see `palette_change_reaches_mesh`), so they
/// follow. Also catches a `TerrainData` replaced wholesale carrying an older
/// palette.
///
/// Written past change detection: the palette is derived state, and marking
/// the data changed would make autosave rewrite the whole terrain.
///
/// A root with terrain layers meshes its bake, so the bake's palette is kept
/// current too, and whether the chunks recolour is judged by the cells the
/// bake holds, layer paint included.
pub fn sync_terrain_slot_palette(
    slots: Res<TerrainMaterialSlots>,
    mut roots: Query<(&TerrainConfig, &mut TerrainData, Option<&mut TerrainBaked>), With<TerrainRoot>>,
    mut dirty: Option<ResMut<TerrainDirtyChunks>>,
) {
    let slots_changed = slots.is_changed();
    for (config, mut data, mut baked) in &mut roots {
        let baked_stale = baked.as_ref().is_some_and(|baked| baked.data.slot_palette != *slots.palette());
        if !slots_changed && !data.is_changed() && !baked_stale {
            continue;
        }
        let base_stale = data.slot_palette != *slots.palette();
        if !base_stale && !baked_stale {
            continue;
        }
        let recolours = palette_change_reaches_mesh(surface_data(&data, baked.as_deref()), slots.palette());
        if base_stale {
            data.bypass_change_detection().slot_palette = slots.palette().clone();
        }
        if let Some(baked) = baked.as_mut().filter(|_| baked_stale) {
            baked.bypass_change_detection().data.slot_palette = slots.palette().clone();
        }
        if recolours {
            if let Some(dirty) = dirty.as_mut() {
                dirty.mark_all_meshes(config);
            }
        }
    }
}

/// Whether swapping `data`'s palette for `next` changes any chunk's vertex
/// colours. A custom slot reaches a mesh only through a material-map cell
/// naming it, so defining a slot no cell uses yet (the Add material action
/// picks such a slot) needs no remesh. Built-in slots always count: brick
/// vertices read them by `TerrainMaterial`, and a point with no material
/// falls back to Grass. The RGB must stay current even for chunks on the
/// textured surface, which fall back to vertex colours when the arrays go away.
fn palette_change_reaches_mesh(data: &TerrainData, next: &TerrainSlotPalette) -> bool {
    let changed: Vec<u8> = (0..MATERIAL_SLOT_NONE)
        .filter(|&slot| data.slot_palette.srgb(slot) != next.srgb(slot))
        .collect();
    if changed.iter().any(|&slot| slot < FIRST_CUSTOM_MATERIAL_SLOT) {
        return true;
    }
    let mut named = [false; MATERIAL_SLOT_COUNT];
    for &[a, b, _, _] in &data.material_cache {
        named[a as usize] = true;
        named[b as usize] = true;
    }
    changed.iter().any(|&slot| named[slot as usize])
}

/// Point the brush back at Grass when the slot table stops defining the slot
/// it paints (a Space switch, a deleted `.mat.toml`), so a stroke never lays
/// down cells nothing draws and a later material in that slot does not take
/// them over.
pub fn keep_paint_material_defined(slots: Res<TerrainMaterialSlots>, brush: Option<ResMut<TerrainBrush>>) {
    if !slots.is_changed() {
        return;
    }
    let Some(mut brush) = brush else {
        return;
    };
    // Read through `Deref` first, so a valid slot leaves the brush unchanged.
    if slots.get(brush.paint_material).is_none() {
        brush.paint_material = TerrainMaterial::Grass.to_u8();
    }
}

/// What a gameplay system gets back from [`TerrainMaterialQuery::material_at`]:
/// the material mix of the ground at a point, with the stronger slot's
/// definition resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainMaterialHit {
    /// The cell's slots and blend (see `height_query::material_at_world`).
    pub sample: MaterialSample,
    /// Display name of the stronger slot, `None` when the slot table does not
    /// define it (a custom slot whose file went missing).
    pub name: Option<String>,
    /// The built-in material the stronger slot is or starts from.
    pub base: Option<TerrainMaterial>,
    /// The realism material the stronger slot's friction comes from.
    pub physics_material: Option<String>,
    /// Static and kinetic friction of the stronger slot, `None` where the
    /// realism registry has no entry for it (see
    /// [`TerrainMaterialSlots::friction`]).
    pub friction: Option<(f32, f32)>,
}

/// Terrain material lookups for gameplay systems (footstep sounds, tyre grip,
/// dust colour): take it as a system parameter and ask what the ground at a
/// point is made of. It reads the one terrain root and the active slot table,
/// and answers `None` without a terrain, without a material layer, or while
/// two roots coexist mid-replacement. A root with terrain layers answers from
/// its bake, so a road's asphalt reads as asphalt.
#[derive(SystemParam)]
pub struct TerrainMaterialQuery<'w, 's> {
    roots: Query<
        'w,
        's,
        (&'static TerrainConfig, &'static TerrainData, Option<&'static TerrainBaked>),
        With<TerrainRoot>,
    >,
    slots: Option<Res<'w, TerrainMaterialSlots>>,
}

impl TerrainMaterialQuery<'_, '_> {
    /// The material mix of the cell at world `world_x, world_z` (metres), as
    /// `height_query::material_at_world` reports it.
    pub fn sample_at(&self, world_x: f32, world_z: f32) -> Option<MaterialSample> {
        let (config, data, baked) = self.roots.single().ok()?;
        material_at_world(config, surface_data(data, baked), world_x, world_z)
    }

    /// [`Self::sample_at`] with the stronger slot's name, base material,
    /// physics material and friction.
    pub fn material_at(&self, world_x: f32, world_z: f32) -> Option<TerrainMaterialHit> {
        let sample = self.sample_at(world_x, world_z)?;
        let slots = self.slots.as_deref();
        let slot = slots.and_then(|slots| slots.get(sample.primary));
        Some(TerrainMaterialHit {
            sample,
            name: slot.map(|slot| slot.name.clone()),
            base: slot.map(|slot| slot.base),
            physics_material: slot.and_then(|slot| slot.physics_material.clone()),
            friction: slots.and_then(|slots| slots.friction(sample.primary)),
        })
    }
}

/// The material slot table, its texture arrays and the systems that keep
/// them current, plus the chunk-collider friction derived from it. Added by
/// the shared `TerrainPlugin` (Client) and by the engine's
/// `EngineTerrainPlugin`; both guard against adding it twice.
pub struct TerrainMaterialSlotsPlugin;

impl Plugin for TerrainMaterialSlotsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TerrainMaterialSlots>()
            .init_resource::<TerrainMaterialSource>()
            // Before the chunk spawner, so a root spawned this frame meshes
            // its first chunks with the Space's palette.
            .add_systems(
                Update,
                (watch_terrain_material_files, reload_terrain_material_slots, sync_terrain_slot_palette)
                    .chain()
                    .before(super::process_terrain_generation_queue),
            );
        super::texture_arrays::register(app);
        app.add_systems(
            Update,
            (
                super::texture_arrays::drive_terrain_texture_arrays.after(sync_terrain_slot_palette),
                keep_paint_material_defined.after(reload_terrain_material_slots),
            ),
        );
        // Registered here rather than next to the collider builders because
        // both hosts share this plugin and it owns the slot table the
        // friction comes from; after the reload, so a slot edit remaps the
        // colliders in the same frame.
        #[cfg(feature = "physics")]
        app.add_systems(
            Update,
            super::collider::apply_terrain_chunk_friction.after(reload_terrain_material_slots),
        );
    }

    fn finish(&self, app: &mut App) {
        super::texture_arrays::finish(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::material::MATERIAL_COUNT;

    fn temp_terrain_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress_material_slots_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("materials")).unwrap();
        dir
    }

    fn bundled(slot: &MaterialSlot) -> Option<&'static str> {
        match slot.texture_set {
            Some(TerrainTextureSet::Bundled(name)) => Some(name),
            _ => None,
        }
    }

    #[test]
    fn the_builtin_table_covers_every_builtin_slot_with_its_texture_set() {
        use TerrainMaterial::*;
        let expected: [(TerrainMaterial, &str); MATERIAL_COUNT] = [
            (Grass, "grass"),
            (Rock, "granite"),
            (Dirt, "sand"),
            (Snow, "ice"),
            (Sand, "sand"),
            (Mud, "sand"),
            (Concrete, "concrete"),
            (Asphalt, "concrete"),
            (Slate, "slate"),
            (Brick, "brick"),
            (WoodPlanks, "wood_planks"),
            (Glacier, "ice"),
            (Sandstone, "sand"),
            (Basalt, "slate"),
            (Ground, "sand"),
            (CrackedLava, "slate"),
            (Cobblestone, "granite"),
            (Ice, "ice"),
            (LeafyGrass, "grass"),
            (Salt, "marble"),
            (Limestone, "marble"),
            (Pavement, "concrete"),
            (Water, "ice"),
        ];
        let table = TerrainMaterialSlots::builtins();
        for (material, set) in expected {
            let slot = table.get(material.to_u8()).unwrap_or_else(|| panic!("{material:?} has no slot"));
            assert_eq!(slot.base, material);
            assert_eq!(slot.name, material.name());
            assert_eq!(bundled(slot), Some(set), "{material:?}");
            assert_eq!(slot.roughness, material.roughness());
            assert!(slot.tiling > 0.0);
            assert!(slot.tint.iter().all(|c| *c > 0.0 && *c <= MAX_TINT), "{material:?} tint {:?}", slot.tint);
            assert!(slot.source.is_none());
        }
        assert_eq!(table.iter().count(), MATERIAL_COUNT, "only the built-ins are defined");
        assert!(table.get(FIRST_CUSTOM_MATERIAL_SLOT).is_none());
        assert!(table.get(MATERIAL_SLOT_NONE).is_none());
        assert_eq!(table.next_unused_custom_slot(&[]), Some(FIRST_CUSTOM_MATERIAL_SLOT));
    }

    #[test]
    fn builtin_swatches_match_their_material_colours() {
        let table = TerrainMaterialSlots::builtins();
        for material in TerrainMaterial::all() {
            let want = material.base_color().to_srgba();
            let got = table.swatch_srgb(material.to_u8());
            for (g, w) in got.iter().zip([want.red, want.green, want.blue]) {
                assert!((g - w).abs() < 2e-3, "{material:?}: swatch {got:?}, base colour {want:?}");
            }
        }
        // Mud is tinted sand, and darker than sand.
        let mud = table.get(TerrainMaterial::Mud.to_u8()).unwrap().swatch_linear();
        let sand = table.get(TerrainMaterial::Sand.to_u8()).unwrap().swatch_linear();
        assert!(mud.iter().zip(sand).all(|(m, s)| *m < s));
        assert_eq!(table.swatch_srgb(200), UNDEFINED_SLOT_SRGB);
        assert_eq!(*table.palette(), TerrainSlotPalette::default());
    }

    #[test]
    fn builtin_physics_names_resolve_in_the_realism_registry() {
        use crate::realism::materials::properties::MaterialProperties;
        for (slot, def) in TerrainMaterialSlots::builtins().iter() {
            if let Some(name) = &def.physics_material {
                assert!(MaterialProperties::from_name(name).is_some(), "slot {slot}: {name:?} does not resolve");
            }
        }
    }

    #[test]
    fn every_bundled_set_names_real_files() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        for (name, mean) in BUNDLED_TEXTURE_SETS {
            let files = TerrainTextureSet::Bundled(name).files(&root);
            assert!(files.albedo.is_file(), "{}", files.albedo.display());
            assert!(files.normal.as_ref().unwrap().is_file());
            assert!(files.orm.as_ref().unwrap().is_file());
            assert!(mean.iter().all(|c| *c > 0.0 && *c <= 1.0));
        }
    }

    #[test]
    fn unique_texture_sets_give_one_layer_per_set_in_first_use_order() {
        let mut table = TerrainMaterialSlots::builtins();
        let names: Vec<String> = table.unique_texture_sets().iter().map(TerrainTextureSet::label).collect();
        assert_eq!(
            names,
            ["grass", "granite", "sand", "ice", "concrete", "slate", "brick", "wood_planks", "marble"]
        );

        // A custom slot on a set already in use adds no layer...
        let mut red_rock = builtin_slot(TerrainMaterial::Rock);
        red_rock.tint = [1.0, 0.6, 0.5];
        table.set_slot(30, Some(red_rock));
        assert_eq!(table.unique_texture_sets().len(), 9);
        // ...one on a new bundled set or on its own files adds one each, and
        // two slots on the same files share theirs.
        let mut gold = builtin_slot(TerrainMaterial::Rock);
        gold.texture_set = Some(TerrainTextureSet::Bundled("gold"));
        table.set_slot(31, Some(gold));
        let files = TerrainTextureSet::Files { albedo: PathBuf::from("/space/a.png"), normal: None, orm: None };
        for slot in [32, 33] {
            let mut own = builtin_slot(TerrainMaterial::Grass);
            own.texture_set = Some(files.clone());
            table.set_slot(slot, Some(own));
        }
        let sets = table.unique_texture_sets();
        assert_eq!(sets.len(), 11);
        assert_eq!(sets[9], TerrainTextureSet::Bundled("gold"));
        assert_eq!(sets[10], files);
        assert!(table.uses_texture_file(Path::new("/space/a.png")));

        // Clearing a built-in restores it; slot 255 never takes a definition.
        table.set_slot(0, None);
        assert_eq!(table.get(0), Some(&builtin_slot(TerrainMaterial::Grass)));
        table.set_slot(MATERIAL_SLOT_NONE, Some(builtin_slot(TerrainMaterial::Rock)));
        assert!(table.get(MATERIAL_SLOT_NONE).is_none());
    }

    #[test]
    fn legacy_palette_files_override_only_what_they_set() {
        let dir = temp_terrain_dir("legacy");
        // What `create_default_terrain_toml` and the exporters write: four
        // built-in palette entries, two of whose files exist with empty
        // texture paths, an explicit roughness and the legacy tiling.
        toml_loader::create_default_terrain_toml(&dir).unwrap();
        let legacy = "[material]\nname = \"Grass\"\nalbedo = \"\"\nnormal = \"\"\n\
                      roughness = 0.5\nmetallic = 0.0\nao = \"\"\ntiling = [8.0, 8.0]\n";
        std::fs::write(dir.join("materials/grass.mat.toml"), legacy).unwrap();
        std::fs::write(dir.join("materials/rock.mat.toml"), "[material]\nname = \"Rock\"\n").unwrap();

        let (table, warnings) = TerrainMaterialSlots::load_from_terrain_dir(&dir);
        assert!(warnings.is_empty(), "{warnings:?}");
        let grass = table.get(0).unwrap();
        let shipped = builtin_slot(TerrainMaterial::Grass);
        assert_eq!(grass.texture_set, shipped.texture_set, "empty paths keep the built-in textures");
        assert_eq!(grass.tint, shipped.tint);
        assert_eq!(grass.tiling, shipped.tiling, "the legacy tiling array is not honoured");
        assert_eq!(grass.roughness, 0.5, "an explicit roughness overrides");
        assert_eq!(grass.source.as_deref(), Some(dir.join("materials/grass.mat.toml").as_path()));
        // A file that sets nothing but its name changes nothing but the source.
        let rock = table.get(1).unwrap();
        assert_eq!(rock.roughness, TerrainMaterial::Rock.roughness());
        assert_eq!(rock.texture_set, builtin_slot(TerrainMaterial::Rock).texture_set);
        // Palette entries whose files do not exist leave the built-in alone.
        assert_eq!(table.get(2), Some(&builtin_slot(TerrainMaterial::Dirt)));
        assert_eq!(table.iter().count(), MATERIAL_COUNT);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn custom_material_files_define_slots_with_and_without_the_new_keys() {
        let dir = temp_terrain_dir("custom");
        std::fs::write(
            dir.join("_terrain.toml"),
            "[terrain]\nchunk_size = 64.0\n\n\
             [[materials.palette]]\nslot = 30\nname = \"Moss\"\nfile = \"materials/moss.mat.toml\"\n\n\
             [[materials.palette]]\nslot = 31\nname = \"Lost\"\nfile = \"materials/lost.mat.toml\"\n",
        )
        .unwrap();
        // Listed by the palette, no new keys at all: a Grass-based slot
        // named after the file, the palette's slot id.
        std::fs::write(dir.join("materials/moss.mat.toml"), "[material]\nname = \"Moss\"\nroughness = 0.9\n").unwrap();
        // Not listed, claiming its own slot with every new key.
        std::fs::write(
            dir.join("materials/red_rock.mat.toml"),
            "[material]\nname = \"Red Rock\"\nslot = 40\nbase = \"rock\"\ntint = [1.2, 0.7, 0.6]\n\
             tiling = 6.0\nphysics_material = \"\"\n",
        )
        .unwrap();
        // Its own maps: relative to the material file.
        std::fs::write(
            dir.join("materials/tiles.mat.toml"),
            "[material]\nname = \"Tiles\"\nslot = 41\nalbedo = \"textures/tiles_albedo.png\"\n\
             orm = \"textures/tiles_orm.png\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("materials/flat.mat.toml"),
            "[material]\nname = \"Flat\"\nslot = 42\nbase = \"Brick\"\ntexture_set = \"none\"\n",
        )
        .unwrap();
        // A second claim on slot 40 loses to the first file in name order.
        std::fs::write(dir.join("materials/x_dupe.mat.toml"), "[material]\nname = \"Dupe\"\nslot = 40\n").unwrap();
        // No slot key and not listed: not a terrain material.
        std::fs::write(dir.join("materials/loose.mat.toml"), "[material]\nname = \"Loose\"\n").unwrap();

        let (table, warnings) = TerrainMaterialSlots::load_from_terrain_dir(&dir);

        let moss = table.get(30).unwrap();
        assert_eq!(moss.name, "Moss");
        assert_eq!(moss.base, TerrainMaterial::Grass);
        assert_eq!(bundled(moss), Some("grass"));
        assert_eq!(moss.roughness, 0.9);
        assert_eq!(moss.tint, builtin_slot(TerrainMaterial::Grass).tint, "no new set, so the base's tint stays");

        // A listed file that does not exist still defines its slot, from the
        // palette entry.
        let lost = table.get(31).unwrap();
        assert_eq!((lost.name.as_str(), lost.base), ("Lost", TerrainMaterial::Grass));
        assert!(lost.source.is_none());

        let red = table.get(40).unwrap();
        assert_eq!(red.name, "Red Rock");
        assert_eq!(red.base, TerrainMaterial::Rock);
        assert_eq!(bundled(red), Some("granite"));
        assert_eq!(red.tint, [1.2, 0.7, 0.6]);
        assert_eq!(red.tiling, 6.0);
        assert_eq!(red.physics_material, None, "an empty physics name clears the base's");
        assert_eq!(red.source.as_deref(), Some(dir.join("materials/red_rock.mat.toml").as_path()));

        let tiles = table.get(41).unwrap();
        assert_eq!(
            tiles.texture_set,
            Some(TerrainTextureSet::Files {
                albedo: dir.join("materials").join("textures/tiles_albedo.png"),
                normal: None,
                orm: Some(dir.join("materials").join("textures/tiles_orm.png")),
            })
        );
        assert_eq!(tiles.tint, [1.0; 3], "its own texture starts untinted");

        let flat = table.get(42).unwrap();
        assert_eq!(flat.texture_set, None);
        let brick = TerrainMaterial::Brick.base_color().to_srgba();
        let swatch = flat.swatch_srgb();
        assert!((swatch[0] - brick.red).abs() < 2e-3 && (swatch[1] - brick.green).abs() < 2e-3);

        assert!(table.get(43).is_none(), "the file without a slot defines nothing");
        assert!(warnings.iter().any(|w| w.contains("x_dupe") && w.contains("already defined")), "{warnings:?}");
        // The custom swatches reach the palette.
        assert_eq!(table.palette().srgb(40), red.swatch_srgb());
        assert_ne!(table.palette().srgb(40), UNDEFINED_SLOT_SRGB);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_written_custom_material_loads_back_into_its_slot() {
        let dir = temp_terrain_dir("write");
        let slot = TerrainMaterialSlots::builtins().next_unused_custom_slot(&[]).unwrap();
        let path = write_custom_material_toml(&dir, slot, "Red \"Canyon\" Rock", TerrainMaterial::Rock).unwrap();
        assert_eq!(path, dir.join("materials/red_canyon_rock.mat.toml"));
        // A second material of the same name does not overwrite the first.
        let again = write_custom_material_toml(&dir, slot + 1, "Red Canyon Rock", TerrainMaterial::Sand).unwrap();
        assert_eq!(again, dir.join("materials/red_canyon_rock_2.mat.toml"));

        let (table, warnings) = TerrainMaterialSlots::load_from_terrain_dir(&dir);
        assert!(warnings.is_empty(), "{warnings:?}");
        let written = table.get(slot).unwrap();
        assert_eq!(written.name, "Red \"Canyon\" Rock");
        assert_eq!(written.base, TerrainMaterial::Rock);
        assert_eq!(written.texture_set, builtin_slot(TerrainMaterial::Rock).texture_set);
        assert_eq!(table.get(slot + 1).unwrap().base, TerrainMaterial::Sand);
        assert_eq!(table.next_unused_custom_slot(&[]), Some(slot + 2));

        assert!(write_custom_material_toml(&dir, 3, "Snowier", TerrainMaterial::Snow).is_err());
        assert!(write_custom_material_toml(&dir, MATERIAL_SLOT_NONE, "None", TerrainMaterial::Snow).is_err());
        assert!(write_custom_material_toml(&dir, 60, "  ", TerrainMaterial::Snow).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_new_material_never_takes_a_file_name_the_palette_lists() {
        let dir = temp_terrain_dir("palette_name");
        // The palette names rock.mat.toml for built-in slot 1, but the file
        // was never written.
        std::fs::write(
            dir.join("_terrain.toml"),
            "[terrain]\nchunk_size = 64.0\n\n\
             [[materials.palette]]\nslot = 1\nname = \"Rock\"\nfile = \"materials/rock.mat.toml\"\n",
        )
        .unwrap();
        let path = write_custom_material_toml(&dir, FIRST_CUSTOM_MATERIAL_SLOT, "Rock", TerrainMaterial::Rock).unwrap();
        assert_eq!(path, dir.join("materials/rock_2.mat.toml"));

        let (table, warnings) = TerrainMaterialSlots::load_from_terrain_dir(&dir);
        assert!(warnings.is_empty(), "{warnings:?}");
        let custom = table.get(FIRST_CUSTOM_MATERIAL_SLOT).expect("the new material defines its slot");
        assert_eq!(custom.base, TerrainMaterial::Rock);
        assert_eq!(custom.source.as_deref(), Some(path.as_path()));
        assert!(table.get(1).unwrap().source.is_none(), "the palette's slot still has no file");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_unused_custom_slot_skips_slots_the_material_map_still_names() {
        use crate::terrain::material::material_cell;
        let first = FIRST_CUSTOM_MATERIAL_SLOT;
        let mut table = TerrainMaterialSlots::builtins();
        assert_eq!(table.next_unused_custom_slot(&[]), Some(first));
        // Cells still painted with two undefined custom slots, one of them as
        // the weaker material of a mix.
        let cells = [material_cell(first), [0, first + 1, 40, 0], material_cell(0)];
        assert_eq!(table.next_unused_custom_slot(&cells), Some(first + 2));
        table.set_slot(first + 2, Some(builtin_slot(TerrainMaterial::Rock)));
        assert_eq!(table.next_unused_custom_slot(&cells), Some(first + 3), "a defined slot is taken");
    }

    #[test]
    fn friction_comes_from_the_physics_material_and_is_none_without_one() {
        use crate::realism::materials::properties::MaterialProperties;
        let mut table = TerrainMaterialSlots::builtins();
        let ice = MaterialProperties::from_name("Ice").unwrap();
        assert_eq!(table.friction(TerrainMaterial::Ice.to_u8()), Some((ice.friction_static, ice.friction_kinetic)));
        assert_eq!(table.friction(TerrainMaterial::Snow.to_u8()), None, "snow has no realism entry");
        assert_eq!(table.friction(200), None, "an undefined slot");
        let mut odd = builtin_slot(TerrainMaterial::Rock);
        odd.physics_material = Some("Unobtainium".to_string());
        table.set_slot(30, Some(odd));
        assert_eq!(table.friction(30), None, "a name the registry does not know");
    }

    #[test]
    fn the_material_query_reports_the_ground_under_a_point() {
        use bevy::ecs::system::RunSystemOnce;
        use crate::terrain::height_query::{ensure_material_cache, paint_material_at_world};

        fn ground_at_painted_point(query: TerrainMaterialQuery) -> Option<TerrainMaterialHit> {
            query.material_at(10.0, 10.0)
        }
        fn ground_far_away(query: TerrainMaterialQuery) -> Option<TerrainMaterialHit> {
            query.material_at(-40.0, -40.0)
        }

        let config = TerrainConfig {
            chunk_size: 64.0,
            chunk_resolution: 16,
            chunks_x: 1,
            chunks_z: 1,
            ..TerrainConfig::default()
        };
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        let mut world = World::new();
        world.insert_resource(TerrainMaterialSlots::builtins());
        assert_eq!(world.run_system_once(ground_at_painted_point).expect("runs"), None, "no terrain yet");

        ensure_material_cache(&mut data);
        let ice = TerrainMaterial::Ice.to_u8();
        paint_material_at_world(&config, &mut data, 10.0, 10.0, ice, 1.0);
        world.spawn((TerrainRoot, config, data));

        let hit = world.run_system_once(ground_at_painted_point).expect("runs").expect("the painted cell");
        assert_eq!(hit.sample.primary, ice);
        assert_eq!(hit.sample.secondary, None);
        assert_eq!(hit.name.as_deref(), Some("Ice"));
        assert_eq!(hit.base, Some(TerrainMaterial::Ice));
        assert_eq!(hit.physics_material.as_deref(), Some("Ice"));
        assert!(hit.friction.is_some());
        assert_eq!(hit.friction, TerrainMaterialSlots::builtins().friction(ice));

        let far = world.run_system_once(ground_far_away).expect("runs").expect("unpainted ground");
        assert_eq!(far.sample.primary, TerrainMaterial::Grass.to_u8());
        assert_eq!(far.name.as_deref(), Some("Grass"));
    }

    #[test]
    fn the_material_query_answers_from_the_layer_bake() {
        use bevy::ecs::system::RunSystemOnce;
        use crate::terrain::height_query::{ensure_material_cache, paint_material_at_world};

        fn ground_at(query: TerrainMaterialQuery) -> Option<u8> {
            query.sample_at(10.0, 10.0).map(|sample| sample.primary)
        }

        let config = TerrainConfig {
            chunk_size: 64.0,
            chunk_resolution: 16,
            chunks_x: 1,
            chunks_z: 1,
            ..TerrainConfig::default()
        };
        let mut base = TerrainData::procedural();
        base.resize_cache(&config);
        ensure_material_cache(&mut base);
        // Paint only the bake, as a road layer would.
        let mut baked = TerrainBaked::new(&base, Vec::new());
        let asphalt = TerrainMaterial::Asphalt.to_u8();
        paint_material_at_world(&config, &mut baked.data, 10.0, 10.0, asphalt, 1.0);
        let mut world = World::new();
        world.spawn((TerrainRoot, config, base, baked));
        assert_eq!(world.run_system_once(ground_at).expect("runs"), Some(asphalt));
    }

    #[test]
    fn the_brush_falls_back_to_grass_when_its_slot_goes_away() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        let mut slots = TerrainMaterialSlots::builtins();
        slots.set_slot(30, Some(builtin_slot(TerrainMaterial::Rock)));
        world.insert_resource(slots);
        world.insert_resource(TerrainBrush { paint_material: 30, ..TerrainBrush::default() });
        world.run_system_once(keep_paint_material_defined).expect("runs");
        assert_eq!(world.resource::<TerrainBrush>().paint_material, 30, "a defined slot stays");

        // The Space's table no longer has slot 30.
        world.insert_resource(TerrainMaterialSlots::builtins());
        world.run_system_once(keep_paint_material_defined).expect("runs");
        assert_eq!(world.resource::<TerrainBrush>().paint_material, TerrainMaterial::Grass.to_u8());
    }

    #[test]
    fn a_directory_without_terrain_files_is_the_builtins() {
        let dir = std::env::temp_dir().join(format!("eustress_material_slots_missing_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (table, warnings) = TerrainMaterialSlots::load_from_terrain_dir(&dir);
        assert!(warnings.is_empty());
        assert_eq!(table, TerrainMaterialSlots::builtins());
    }
}
