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
    "Spruce", "Fir", "Pine", "Redwood", "Sequoia", "Mangrove",
    "Bulrush", "Sedge", "Reed", "Rush", "Cattail", "Marsh Marigold",
    "Meadowsweet", "Yarrow", "Tansy", "Chicory", "Comfrey", "Borage",
    "Chamomile", "Lavender", "Rosemary", "Thyme", "Sage", "Marjoram",
    "Fennel", "Dill", "Angelica", "Lovage", "Mallow", "Foxglove",
    "Columbine", "Larkspur", "Delphinium", "Lupine", "Snapdragon",
    "Hollyhock", "Cornflower", "Poppy", "Bluebell", "Harebell",
    "Campion", "Primrose", "Cowslip", "Violet", "Periwinkle", "Speedwell",
    "Forget-me-not", "Anemone", "Celandine", "Buttercup", "Trefoil",
    "Sainfoin", "Lucerne", "Timothy", "Fescue", "Ryegrass", "Bentgrass",
    "Barley", "Millet", "Sorghum", "Amaranth", "Buckwheat", "Flax",
    "Hemp", "Nettle", "Burdock", "Dock", "Plantain", "Dandelion",
    "Coltsfoot", "Butterbur", "Hogweed", "Cow Parsley", "Hedgerow",
    "Bramble", "Briar", "Sloe", "Rosehip", "Elderflower", "Hazelnut",
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
