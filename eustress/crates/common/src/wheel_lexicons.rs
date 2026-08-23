//! # Per-wheel swatch lexicons
//!
//! Each of the seven color wheels names its 127 swatches from its own lexicon.
//! The wheels share a geometry (the same hexagon honeycomb, the same 127 cell
//! positions) and a perceptual source (the curated `BASE_PALETTE`), so the
//! lexicon is what actually distinguishes one wheel from another to a user:
//! picking `Umbra -> Andromalius` and picking `Halo -> Mumiah` land on the same
//! cell of the same grid, and mean entirely different things.
//!
//! ```text
//!   wheel      axis position       lexicon                     numerology
//!   Aether     abstract · good     virtues, radiant concepts   gematria of name
//!   Halo       light · good        angels and guardians        the 72 Shem HaMephorash
//!   Verdure    realistic · good    flora, growth, living world rank by frequency
//!   Stone      neutral centre      minerals and materials      plain index
//!   Char       realistic · evil    scars, ash, suffering       rank by frequency
//!   Hex        abstract · evil     curses, afflictions, chaos  gematria of name
//!   Umbra      dark · evil         demons (Ars Goetia)         the 72 demons
//! ```
//!
//! Halo and Umbra are deliberately mirrored: the first 72 entries of each are
//! the canonical 72 angels of the Shem HaMephorash and the 72 demons of the Ars
//! Goetia, in their traditional order, so index `n` on one wheel is the
//! counterpart of index `n` on the other. The remaining 55 entries of each
//! continue the theme without claiming canonical status.
//!
//! `Stone` has no entry here on purpose. It renders [`crate::color_wheels`]'s
//! `BASE_PALETTE` verbatim, names included, and is the reference the other six
//! transform away from.

use crate::brick_palette::Wheel;

/// Swatches per wheel. Fixed by the honeycomb layout (rows 7 -> 13 -> 7).
pub const WHEEL_SWATCHES: usize = 127;

/// The lexicon for `wheel`, or `None` for [`Wheel::Stone`], which keeps the
/// curated base names.
pub fn wheel_lexicon(wheel: Wheel) -> Option<&'static [&'static str; WHEEL_SWATCHES]> {
    match wheel {
        Wheel::Stone => None,
        Wheel::Aether => Some(&AETHER),
        Wheel::Halo => Some(&HALO),
        Wheel::Verdure => Some(&VERDURE),
        Wheel::Char => Some(&CHAR),
        Wheel::Hex => Some(&HEX),
        Wheel::Umbra => Some(&UMBRA),
    }
}

/// The numerology value shown beside a swatch, per the wheel's scheme.
///
/// * `Aether` / `Hex` — gematria of the name (a=1..z=26, summed).
/// * `Halo` / `Umbra` — the canonical 1..=72 rank for the first 72 entries;
///   entries past the mirror carry their plain index.
/// * `Verdure` / `Char` — rank by usage frequency, which the palette study
///   fills in later; until then the index is the stable stand-in.
/// * `Stone` — plain index.
pub fn numerology(wheel: Wheel, index: usize, name: &str) -> u32 {
    match wheel {
        Wheel::Aether | Wheel::Hex => gematria(name),
        _ => index as u32 + 1,
    }
}

// ============================================================================
// Angelic correspondences — the ruling color of each Shem HaMephorash angel
// ============================================================================

/// The twelve zodiac signs, in order from Aries.
pub const ZODIAC: [&str; 12] = [
    "Aries", "Taurus", "Gemini", "Cancer", "Leo", "Virgo",
    "Libra", "Scorpio", "Sagittarius", "Capricorn", "Aquarius", "Pisces",
];

/// The nine choirs, in order, eight angels each.
pub const CHOIRS: [&str; 9] = [
    "Seraphim", "Cherubim", "Thrones", "Dominions", "Powers",
    "Virtues", "Principalities", "Archangels", "Angels",
];

/// The sephira / planetary ruler traditionally paired with each choir, in the
/// same order as [`CHOIRS`]. Carried for display; the swatch takes only the
/// saturation weighting below.
pub const CHOIR_RULERS: [&str; 9] = [
    "Chokmah · Neptune", "Binah · Saturn", "Chesed · Jupiter",
    "Geburah · Mars", "Tiphareth · Sol", "Netzach · Venus",
    "Hod · Mercury", "Yesod · Luna", "Malkuth · Earth",
];

/// Hue in degrees for each sign, following the Golden Dawn King Scale rather
/// than an even 30 degree split — the traditional scale is compressed through
/// the warm quadrant and stretched through blue, which is what makes the ring
/// read as the zodiac and not as a plain rainbow.
const SIGN_HUE: [f32; 12] = [
    0.0,   // Aries        scarlet
    25.0,  // Taurus       red-orange
    45.0,  // Gemini       orange
    60.0,  // Cancer       amber
    75.0,  // Leo          yellow
    90.0,  // Virgo        yellow-green
    120.0, // Libra        emerald
    165.0, // Scorpio      green-blue
    210.0, // Sagittarius  blue
    250.0, // Capricorn    indigo
    280.0, // Aquarius     violet
    320.0, // Pisces       crimson
];

/// Saturation weighting per choir, highest nearest the source. The choir sets
/// how PURE the ruling hue reads; the hue itself comes from the sign.
const CHOIR_SATURATION: [f32; 9] = [
    0.95, // Seraphim
    0.88, // Cherubim
    0.82, // Thrones
    0.76, // Dominions
    0.70, // Powers
    0.64, // Virtues
    0.58, // Principalities
    0.52, // Archangels
    0.46, // Angels
];

/// Where angel `rank` (1..=72) sits in the system: sign, choir, and the degrees
/// of the zodiac it governs.
///
/// Each angel rules 5 degrees, in sequence from 0 Aries, so six fall in every
/// sign and eight in every choir. Returns `None` outside 1..=72 — the Halo
/// wheel's entries past 72 are thematic, not canonical, and carry no rulership.
pub fn angel_rulership(rank: u32) -> Option<AngelRulership> {
    if rank == 0 || rank > 72 {
        return None;
    }
    let i = (rank - 1) as usize;
    Some(AngelRulership {
        rank,
        sign: ZODIAC[i / 6],
        sign_index: i / 6,
        choir: CHOIRS[i / 8],
        choir_ruler: CHOIR_RULERS[i / 8],
        choir_index: i / 8,
        degrees_start: (i as f32) * 5.0,
    })
}

/// One angel's place in the zodiacal / choral scheme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AngelRulership {
    pub rank: u32,
    pub sign: &'static str,
    pub sign_index: usize,
    pub choir: &'static str,
    pub choir_ruler: &'static str,
    pub choir_index: usize,
    /// Absolute ecliptic longitude where this angel's 5 degrees begin.
    pub degrees_start: f32,
}

/// The ruling color of angel `rank` (1..=72) as sRGB, at a given `lightness`.
///
/// Hue is the sign's King Scale hue, saturation is the choir's purity. The
/// caller supplies lightness so the wheel can still brighten toward its centre
/// — identity comes from rulership, elevation from position.
///
/// Ranks outside 1..=72 return `None`; the Halo wheel falls back to its
/// generic light transform for those.
pub fn angel_ruling_color(rank: u32, lightness: f32) -> Option<[u8; 3]> {
    let r = angel_rulership(rank)?;
    Some(hsl_to_srgb(
        SIGN_HUE[r.sign_index],
        CHOIR_SATURATION[r.choir_index],
        lightness.clamp(0.0, 1.0),
    ))
}

/// Minimal HSL -> sRGB. Local copy so the correspondence table does not depend
/// on `color_wheels`' private helpers.
fn hsl_to_srgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [
        (((r + m) * 255.0).round().clamp(0.0, 255.0)) as u8,
        (((g + m) * 255.0).round().clamp(0.0, 255.0)) as u8,
        (((b + m) * 255.0).round().clamp(0.0, 255.0)) as u8,
    ]
}

/// Simple English gematria: `a`=1 through `z`=26, summed over the name.
/// Non-letters are skipped, so `Nith-Haiah` scores the same as `NithHaiah`.
pub fn gematria(name: &str) -> u32 {
    name.chars()
        .filter(|c| c.is_ascii_alphabetic())
        .map(|c| (c.to_ascii_lowercase() as u32) - ('a' as u32) + 1)
        .sum()
}

// ============================================================================
// Aether — abstract · good. Virtues and radiant concepts.
// ============================================================================

const AETHER: [&str; WHEEL_SWATCHES] = [
    "Grace", "Valor", "Mercy", "Wonder", "Clarity", "Ardor", "Zeal",
    "Hope", "Solace", "Candor", "Vigil", "Kindle", "Aurora", "Radiance",
    "Prism", "Halcyon", "Lucid", "Beacon", "Verity", "Kindred", "Aspire",
    "Elation", "Rapture", "Marvel", "Reverie", "Epiphany", "Insight",
    "Accord", "Amity", "Nectar", "Zenith", "Apex", "Vantage", "Ascent",
    "Uplift", "Buoyant", "Levity", "Effulgence", "Lumen", "Corona",
    "Glimmer", "Shimmer", "Coruscate", "Scintilla", "Spark", "Flare",
    "Nova", "Quasar", "Pulsar", "Ion", "Plasma", "Photon", "Quantum",
    "Cascade", "Crescendo", "Fanfare", "Ovation", "Jubilee", "Triumph",
    "Laurel", "Garland", "Emblem", "Sigil", "Talisman", "Amulet",
    "Benison", "Boon", "Bounty", "Largesse", "Munificence", "Charity",
    "Compassion", "Empathy", "Devotion", "Fidelity", "Constancy",
    "Temperance", "Prudence", "Fortitude", "Justice", "Wisdom", "Faith",
    "Charisma", "Eloquence", "Rhapsody", "Sonnet", "Cadence", "Harmony",
    "Consonance", "Resonance", "Euphony", "Chorale", "Anthem", "Canticle",
    "Serenade", "Aubade", "Nocturne", "Elysium", "Arcadia", "Empyrean",
    "Firmament", "Ether", "Zephyr", "Sylph", "Aeon", "Infinity",
    "Continuum", "Meridian", "Equinox", "Solstice", "Dawnlight",
    "Daybreak", "Sunspire", "Skyglass", "Cloudsilver", "Starlace",
    "Moonwake", "Comet", "Meteor", "Astral", "Celestine", "Seraphic",
    "Numinous", "Ineffable", "Sublime", "Transcend", "Apotheosis",
];

// ============================================================================
// Halo — light · good. The 72 angels of the Shem HaMephorash, then guardians.
// ============================================================================

const HALO: [&str; WHEEL_SWATCHES] = [
    // 1..=72 — the Shem HaMephorash, traditional order.
    "Vehuiah", "Jeliel", "Sitael", "Elemiah", "Mahasiah", "Lelahel",
    "Achaiah", "Cahetel", "Haziel", "Aladiah", "Lauviah", "Hahaiah",
    "Iezalel", "Mebahel", "Hariel", "Hakamiah", "Loviah", "Caliel",
    "Leuviah", "Pahaliah", "Nelchael", "Yeiayel", "Melahel", "Haheuiah",
    "Nith-Haiah", "Haaiah", "Yerathel", "Seheiah", "Reiyel", "Omael",
    "Lecabel", "Vasariah", "Yehuiah", "Lehahiah", "Chavakiah", "Menadel",
    "Aniel", "Haamiah", "Rehael", "Ieiazel", "Hahahel", "Mikael",
    "Veuliah", "Yelahiah", "Sealiah", "Ariel", "Asaliah", "Mihael",
    "Vehuel", "Daniel", "Hahasiah", "Imamiah", "Nanael", "Nithael",
    "Mebahiah", "Poyel", "Nemamiah", "Yeialel", "Harahel", "Mitzrael",
    "Umabel", "Iah-Hel", "Anauel", "Mehiel", "Damabiah", "Manakel",
    "Eyael", "Habuhiah", "Rochel", "Jabamiah", "Haiaiel", "Mumiah",
    // 73..=127 — archangels, choirs, and guardian concepts.
    "Metatron", "Sandalphon", "Michael", "Gabriel", "Raphael", "Uriel",
    "Raguel", "Remiel", "Sariel", "Raziel", "Zadkiel", "Camael",
    "Haniel", "Jophiel", "Zaphkiel", "Barachiel", "Jehudiel",
    "Selaphiel", "Chamuel", "Azrael", "Cassiel", "Zachariel", "Muriel",
    "Verchiel", "Hamaliel", "Zuriel", "Adnachiel", "Ambriel", "Asariel",
    "Israfel", "Ophaniel", "Seraphiel", "Kerubiel", "Galgaliel",
    "Zagzagel", "Radueriel", "Shamsiel", "Zophiel", "Yahoel", "Anafiel",
    "Seraphim", "Cherubim", "Thrones", "Dominion", "Virtue", "Power",
    "Principality", "Watcher", "Herald", "Warden", "Sentinel",
    "Intercessor", "Psalm", "Vespers", "Matins",
];

// ============================================================================
// Verdure — realistic · good. Flora, growth, the living world.
// ============================================================================

const VERDURE: [&str; WHEEL_SWATCHES] = [
    "Fern", "Moss", "Bracken", "Sorrel", "Thistle", "Clover", "Vetch",
    "Willow", "Alder", "Birch", "Rowan", "Hawthorn", "Blackthorn",
    "Hazel", "Elder", "Aspen", "Linden", "Sycamore", "Chestnut",
    "Hornbeam", "Beech", "Larch", "Cedar", "Juniper", "Cypress", "Yew",
    "Spruce", "Fir", "Pine", "Cedarwood", "Sequoia", "Mangrove",
    "Bulrush", "Sedge", "Reed", "Rush", "Cattail", "Marsh Marigold",
    "Meadowsweet", "Yarrow", "Tansy", "Chervil", "Comfrey", "Purslane",
    "Chamomile", "Lovage", "Rosemary", "Thyme", "Sage", "Marjoram",
    "Fennel", "Dill", "Angelica", "Lambsquarter", "Mallow", "Fiddlehead",
    "Cotyledon Leaf", "Lemongrass", "Duckweed", "Liverleaf", "Samphire",
    "Houseleek", "Coriander", "Pennywort", "Bilberry Leaf", "Hartstongue",
    "Caraway", "Pipsissewa", "Cleavers", "Vervain", "Pellitory", "Spleenwort",
    "Feverfew", "Agrimony", "Chickweed", "Burnet", "Trefoil",
    "Sainfoin", "Lucerne", "Timothy", "Fescue", "Ryegrass", "Bentgrass",
    "Barley", "Millet", "Sorghum", "Amaranth", "Buckwheat", "Flax",
    "Hemp", "Nettle", "Burdock", "Dock", "Plantain", "Dandelion",
    "Coltsfoot", "Butterbur", "Hogweed", "Cow Parsley", "Hedgerow",
    "Bramble", "Briar", "Sloe", "Rockcress", "Eyebright", "Hazelnut",
    "Acorn", "Catkin", "Sapling", "Seedling", "Cotyledon", "Tendril",
    "Rhizome", "Taproot", "Heartwood", "Sapwood", "Cambium", "Bark",
    "Lichen", "Liverwort", "Hornwort", "Frond", "Canopy", "Understory",
    "Loam", "Humus", "Mycelium",
];

// ============================================================================
// Char — realistic · evil. Scars, ash, the realism of suffering.
// ============================================================================

const CHAR: [&str; WHEEL_SWATCHES] = [
    "Cinder", "Soot", "Ashfall", "Scar", "Blister", "Ember", "Slag",
    "Clinker", "Charcoal", "Smoulder", "Scorch", "Sear", "Singe",
    "Blacken", "Tarnish", "Rust", "Corrosion", "Patina", "Verdigris",
    "Oxide", "Scale", "Pitting", "Fracture", "Splinter", "Shard",
    "Gouge", "Abrasion", "Laceration", "Contusion", "Welt", "Bruise",
    "Callus", "Cicatrix", "Keloid", "Suture", "Grist", "Grit", "Chaff",
    "Dross", "Residue", "Sediment", "Silt", "Sludge", "Tailings",
    "Cinderfall", "Coalface", "Firedamp", "Blackdamp", "Choke", "Smoke",
    "Fume", "Reek", "Acrid", "Bitumen", "Tar", "Pitch", "Creosote",
    "Naphtha", "Asphalt", "Slate", "Shale", "Flint", "Basalt", "Obsidian",
    "Pumice", "Tephra", "Lahar", "Caldera", "Fumarole", "Sulphur",
    "Brimstone", "Cauterize", "Brand", "Iron", "Anvil", "Forge", "Bellows",
    "Quench", "Temper", "Hammerfall", "Splint", "Tourniquet", "Poultice",
    "Bandage", "Gauze", "Lint", "Tallow", "Rendered", "Marrow", "Sinew",
    "Gristle", "Hide", "Tannin", "Leather", "Hessian", "Sackcloth",
    "Burlap", "Canvas", "Rope", "Hemp", "Frayed", "Threadbare", "Worn",
    "Weathered", "Bleached", "Sun-cracked", "Parched", "Drought",
    "Famine", "Husk", "Chalkdust", "Bonemeal", "Kiln", "Ashpit",
    "Hearthstone", "Flue", "Chimney", "Soot-fall", "Grate", "Coalbed",
    "Slagheap", "Spoil", "Waste", "Cull", "Remnant", "Vestige", "Scoria",
];

// ============================================================================
// Hex — abstract · evil. Curses, afflictions, chaos.
// ============================================================================

const HEX: [&str; WHEEL_SWATCHES] = [
    "Wormwood", "Blight", "Malice", "Rancor", "Venom", "Bane", "Curse",
    "Fester", "Wither", "Miasma", "Contagion", "Pestilence", "Murrain",
    "Canker", "Ruin", "Rot", "Corruption", "Taint", "Defile", "Profane",
    "Anathema", "Malediction", "Imprecation", "Jinx", "Blightword",
    "Evil Eye", "Scourge", "Affliction", "Torment", "Anguish", "Dread",
    "Terror", "Panic", "Frenzy", "Delirium", "Mania", "Hysteria",
    "Paranoia", "Obsession", "Compulsion", "Fixation", "Vertigo",
    "Nausea", "Migraine", "Neuralgia", "Spasm", "Palsy", "Tremor",
    "Convulsion", "Seizure", "Rictus", "Grimace", "Snarl", "Spite",
    "Venomweave", "Nettlesting", "Thornbite", "Barb", "Sting", "Lash",
    "Flay", "Rend", "Sunder", "Shatter", "Splinterheart", "Discord",
    "Cacophony", "Dissonance", "Static", "Interference", "Distortion",
    "Warp", "Skew", "Fracture", "Rift", "Schism", "Sever", "Unmake",
    "Undo", "Unravel", "Fray", "Erode", "Dissolve", "Corrode",
    "Putrefy", "Necrosis", "Gangrene", "Sepsis", "Toxin", "Alkaloid",
    "Belladonna", "Hemlock", "Nightshade", "Henbane", "Mandrake",
    "Aconite", "Foxbane", "Wolfsbane", "Monkshood", "Digitalis",
    "Strychnine", "Cyanide", "Arsenic", "Verdigris", "Vitriol", "Caustic",
    "Lye", "Quicklime", "Solvent", "Reagent", "Catalyst", "Sublimate",
    "Precipitate", "Effluent", "Runoff", "Leachate", "Slurry", "Bilge",
    "Ichor", "Bile", "Rancid", "Acerbic", "Acrid", "Caustic Bloom",
    "Hexroot", "Bindweed", "Chokevine",
];

// ============================================================================
// Umbra — dark · evil. The 72 demons of the Ars Goetia, then the deeper dark.
// ============================================================================

const UMBRA: [&str; WHEEL_SWATCHES] = [
    // 1..=72 — the Ars Goetia, traditional order. Mirrors HALO's first 72.
    "Bael", "Agares", "Vassago", "Samigina", "Marbas", "Valefor", "Amon",
    "Barbatos", "Paimon", "Buer", "Gusion", "Sitri", "Beleth", "Leraje",
    "Eligos", "Zepar", "Botis", "Bathin", "Sallos", "Purson", "Marax",
    "Ipos", "Aim", "Naberius", "Glasya-Labolas", "Bune", "Ronove",
    "Berith", "Astaroth", "Forneus", "Foras", "Asmoday", "Gaap",
    "Furfur", "Marchosias", "Stolas", "Phenex", "Halphas", "Malphas",
    "Raum", "Focalor", "Vepar", "Sabnock", "Shax", "Vine", "Bifrons",
    "Vual", "Haagenti", "Crocell", "Furcas", "Balam", "Alloces", "Caim",
    "Murmur", "Orobas", "Gremory", "Ose", "Amy", "Orias", "Vapula",
    "Zagan", "Valac", "Andras", "Haures", "Andrealphus", "Cimeies",
    "Amdusias", "Belial", "Decarabia", "Seere", "Dantalion",
    "Andromalius",
    // 73..=127 — the deeper dark: princes, voids, and the unlit spectrum.
    "Lucifer", "Beelzebub", "Leviathan", "Mammon", "Belphegor",
    "Abaddon", "Moloch", "Baphomet", "Lilith", "Naamah", "Azazel",
    "Samael", "Mephisto", "Behemoth", "Nybbas", "Xaphan", "Verrine",
    "Sonneillon", "Rosier", "Gressil", "Carreau", "Oeillet", "Adramelech",
    "Nergal", "Apollyon", "Tenebrae", "Erebus", "Nyx", "Acheron",
    "Cocytus", "Phlegethon", "Styx", "Lethe", "Abyss", "Chasm", "Maw",
    "Nadir", "Eclipse", "Umbral", "Penumbra", "Nightfall", "Gloaming",
    "Dusk", "Pitch", "Sable", "Obsidian Deep", "Starless", "Moonless",
    "Voidlight", "Event Horizon", "Singularity", "Null", "Cipher",
    "Oblivion", "Silence",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_wheel_but_stone_has_a_lexicon() {
        for w in Wheel::ALL {
            match w {
                Wheel::Stone => assert!(wheel_lexicon(w).is_none()),
                _ => assert!(wheel_lexicon(w).is_some(), "wheel {:?}", w),
            }
        }
    }

    /// The arrays are fixed-size so a miscount is a compile error, not a
    /// runtime surprise. This asserts the honeycomb agrees with that size.
    #[test]
    fn lexicons_match_the_honeycomb_cell_count() {
        assert_eq!(WHEEL_SWATCHES, 127);
        for w in Wheel::ALL {
            if let Some(lex) = wheel_lexicon(w) {
                assert_eq!(lex.len(), WHEEL_SWATCHES, "wheel {:?}", w);
            }
        }
    }

    /// A duplicate name inside one wheel would make two different swatches
    /// indistinguishable in the picker and in a saved BrickColor token.
    #[test]
    fn no_duplicate_names_within_a_wheel() {
        for w in Wheel::ALL {
            let Some(lex) = wheel_lexicon(w) else { continue };
            let mut seen = std::collections::HashSet::new();
            for name in lex.iter() {
                assert!(
                    seen.insert(*name),
                    "wheel {:?} repeats the name {:?}",
                    w,
                    name
                );
            }
        }
    }

    #[test]
    fn no_empty_names() {
        for w in Wheel::ALL {
            let Some(lex) = wheel_lexicon(w) else { continue };
            for (i, name) in lex.iter().enumerate() {
                assert!(!name.trim().is_empty(), "wheel {:?} index {}", w, i);
            }
        }
    }

    /// Halo and Umbra are mirrors: the first 72 of each are the canonical
    /// angels and demons, so those ranks must line up one to one.
    #[test]
    fn halo_and_umbra_mirror_across_seventy_two() {
        assert_eq!(HALO[0], "Vehuiah");
        assert_eq!(UMBRA[0], "Bael");
        assert_eq!(HALO[71], "Mumiah");
        assert_eq!(UMBRA[71], "Andromalius");
        // Rank n on one wheel is the counterpart of rank n on the other.
        for i in 0..72 {
            assert_eq!(numerology(Wheel::Halo, i, HALO[i]), i as u32 + 1);
            assert_eq!(numerology(Wheel::Umbra, i, UMBRA[i]), i as u32 + 1);
        }
    }

    #[test]
    fn gematria_sums_letters_and_ignores_punctuation() {
        assert_eq!(gematria("a"), 1);
        assert_eq!(gematria("z"), 26);
        assert_eq!(gematria("abc"), 6);
        // Hyphens and spaces are skipped, so the two spellings agree.
        assert_eq!(gematria("Nith-Haiah"), gematria("NithHaiah"));
        assert_eq!(gematria("Evil Eye"), gematria("EvilEye"));
    }

    /// Aether and Hex are the gematria wheels; the rest rank by position.
    #[test]
    fn numerology_follows_the_wheel_scheme() {
        assert_eq!(numerology(Wheel::Aether, 0, "Grace"), gematria("Grace"));
        assert_eq!(numerology(Wheel::Hex, 5, "Bane"), gematria("Bane"));
        assert_eq!(numerology(Wheel::Verdure, 4, "Thistle"), 5);
        assert_eq!(numerology(Wheel::Stone, 9, "whatever"), 10);
    }
}
