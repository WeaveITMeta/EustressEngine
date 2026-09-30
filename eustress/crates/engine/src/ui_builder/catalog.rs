//! The UI Builder's presets: styles, HUD kits, palettes and genres.
//!
//! - A **style** is the overall direction (Arena, Horror, ...). It picks a
//!   default kit, palette and fonts, and words for the design brief.
//! - A **HUD kit** is how every HUD plate is drawn: its fill, outline,
//!   corner radius and shadow. Switching the kit restyles every element at
//!   once, in the current palette.
//! - A **palette** is five colours with fixed jobs (see
//!   [`super::blueprint::Palette`]).
//! - A **genre** is what the offline generator recognises in a prompt; it
//!   decides the HUD elements and the screens.
//!
//! Everything a kit draws is something the Studio overlay and Play draw:
//! fills, outlines and `corner_radius` on the element itself. UICorner,
//! UIStroke and UIGradient are stored but not drawn yet, so the kits do not
//! use them.

use super::blueprint::{Palette, Rgb};

// ============================================================================
// Palettes
// ============================================================================

pub struct PaletteDef {
    pub id: &'static str,
    pub label: &'static str,
    pub primary: Rgb,
    pub secondary: Rgb,
    pub accent: Rgb,
    pub dark: Rgb,
    pub light: Rgb,
}

impl PaletteDef {
    pub fn to_palette(&self) -> Palette {
        Palette {
            id: self.id.to_string(),
            label: self.label.to_string(),
            primary: self.primary,
            secondary: self.secondary,
            accent: self.accent,
            dark: self.dark,
            light: self.light,
        }
    }
}

pub const PALETTES: &[PaletteDef] = &[
    PaletteDef { id: "arena_night", label: "Arena Night", primary: [255, 170, 40], secondary: [232, 64, 72], accent: [64, 156, 255], dark: [18, 22, 34], light: [255, 255, 255] },
    PaletteDef { id: "neon_pulse", label: "Neon Pulse", primary: [0, 229, 255], secondary: [176, 64, 255], accent: [255, 56, 200], dark: [10, 8, 24], light: [240, 240, 255] },
    PaletteDef { id: "candy_pop", label: "Candy Pop", primary: [255, 116, 186], secondary: [150, 112, 255], accent: [255, 214, 84], dark: [58, 30, 78], light: [255, 250, 255] },
    PaletteDef { id: "meadow", label: "Meadow", primary: [104, 196, 86], secondary: [255, 196, 76], accent: [118, 188, 240], dark: [38, 58, 40], light: [250, 250, 236] },
    PaletteDef { id: "treasure", label: "Treasure", primary: [238, 186, 58], secondary: [172, 58, 42], accent: [70, 146, 124], dark: [42, 30, 22], light: [255, 244, 220] },
    PaletteDef { id: "blood_moon", label: "Blood Moon", primary: [196, 30, 42], secondary: [118, 20, 30], accent: [250, 160, 60], dark: [16, 8, 10], light: [240, 226, 220] },
    PaletteDef { id: "hotel_brass", label: "Hotel Brass", primary: [212, 176, 92], secondary: [142, 40, 52], accent: [132, 200, 238], dark: [22, 20, 20], light: [240, 232, 214] },
    PaletteDef { id: "frost", label: "Frost", primary: [120, 200, 255], secondary: [84, 124, 222], accent: [226, 248, 255], dark: [16, 28, 44], light: [240, 250, 255] },
    PaletteDef { id: "field_ops", label: "Field Ops", primary: [255, 180, 40], secondary: [92, 112, 88], accent: [122, 222, 122], dark: [16, 20, 18], light: [226, 232, 220] },
    PaletteDef { id: "ocean", label: "Deep Ocean", primary: [40, 162, 222], secondary: [22, 92, 162], accent: [250, 210, 90], dark: [8, 24, 40], light: [236, 248, 255] },
    PaletteDef { id: "sunset", label: "Sunset", primary: [255, 122, 70], secondary: [230, 70, 112], accent: [255, 202, 92], dark: [40, 20, 40], light: [255, 240, 230] },
    PaletteDef { id: "mono", label: "Mono", primary: [236, 236, 236], secondary: [120, 120, 120], accent: [255, 210, 60], dark: [14, 14, 14], light: [255, 255, 255] },
];

pub fn palette(id: &str) -> Option<&'static PaletteDef> {
    PALETTES.iter().find(|p| p.id == id)
}

// ============================================================================
// HUD kits
// ============================================================================

/// What a kit fills its plates with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Plate {
    /// The palette's dark colour, see-through by `alpha`.
    Dark,
    /// A deep shade of the palette's primary colour.
    DeepPrimary,
    /// The palette's light colour: pale plates, dark text.
    Light,
    /// White at low opacity over the world.
    Glass,
    /// Warm paper.
    Parchment,
    /// Pure black.
    Black,
    /// No plate at all: the text stands on the world with an outline.
    Clear,
}

/// What a kit draws its outline in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outline {
    None,
    /// The palette's dark colour, near black.
    Ink,
    Primary,
    Accent,
    /// White at low opacity.
    Frost,
    /// The palette's light colour.
    Light,
    /// Dark brown, for paper.
    Brown,
}

pub struct KitDef {
    pub id: &'static str,
    pub label: &'static str,
    /// One line for the Design tab, "Arcade: ..." style.
    pub about: &'static str,
    pub plate: Plate,
    /// Plate opacity, 0 to 1.
    pub alpha: f32,
    pub outline: Outline,
    pub outline_px: f32,
    pub radius: f32,
    /// A hard drop shadow: a dark copy of the plate 4 px down.
    pub shadow: bool,
    /// A 4 px stripe in the accent colour down the plate's left edge.
    pub stripe: bool,
    /// Captions in capitals.
    pub upper: bool,
    /// Overrides the blueprint's body font; empty keeps it.
    pub font: &'static str,
}

pub const KITS: &[KitDef] = &[
    KitDef { id: "auto", label: "Auto", about: "The style's own kit.", plate: Plate::Dark, alpha: 0.78, outline: Outline::None, outline_px: 0.0, radius: 8.0, shadow: false, stripe: false, upper: false, font: "" },
    KitDef { id: "original", label: "Original", about: "Dark rounded plates that sit quietly over any world.", plate: Plate::Dark, alpha: 0.78, outline: Outline::None, outline_px: 0.0, radius: 8.0, shadow: false, stripe: false, upper: false, font: "" },
    KitDef { id: "competitive", label: "Competitive", about: "Shooters and ranked modes: flat slate plates, an accent stripe, nothing that moves.", plate: Plate::Dark, alpha: 0.9, outline: Outline::None, outline_px: 0.0, radius: 2.0, shadow: false, stripe: true, upper: true, font: "GothamBold" },
    KitDef { id: "arcade", label: "Arcade", about: "Arsenal and party games: deep coloured plates, bold outlines, hard shadows.", plate: Plate::DeepPrimary, alpha: 1.0, outline: Outline::Ink, outline_px: 3.0, radius: 10.0, shadow: true, stripe: false, upper: true, font: "GothamBlack" },
    KitDef { id: "glass", label: "Glass", about: "Frosted panes with a thin light edge; the world shows through.", plate: Plate::Glass, alpha: 0.16, outline: Outline::Frost, outline_px: 1.0, radius: 14.0, shadow: false, stripe: false, upper: false, font: "" },
    KitDef { id: "chunky_toy", label: "Chunky Toy", about: "Pet Simulator-style: cream plates, thick ink outlines, big rounded type.", plate: Plate::Light, alpha: 1.0, outline: Outline::Ink, outline_px: 3.0, radius: 16.0, shadow: true, stripe: false, upper: false, font: "FredokaOne" },
    KitDef { id: "minimal", label: "Minimal", about: "No plates: outlined text straight on the world, for horror and walking games.", plate: Plate::Clear, alpha: 0.0, outline: Outline::None, outline_px: 0.0, radius: 0.0, shadow: false, stripe: false, upper: true, font: "" },
    KitDef { id: "neon", label: "Neon", about: "Near-black plates edged in the primary colour, for synthwave and sci-fi.", plate: Plate::Dark, alpha: 0.86, outline: Outline::Primary, outline_px: 2.0, radius: 6.0, shadow: false, stripe: false, upper: true, font: "" },
    KitDef { id: "tactical", label: "Tactical", about: "Square corners, thin accent lines and capitals, like a heads-up display.", plate: Plate::Dark, alpha: 0.72, outline: Outline::Accent, outline_px: 1.0, radius: 0.0, shadow: false, stripe: false, upper: true, font: "RobotoMono" },
    KitDef { id: "storybook", label: "Storybook", about: "RPGs and adventures: parchment plates, brown ink edges, serif type.", plate: Plate::Parchment, alpha: 1.0, outline: Outline::Brown, outline_px: 2.0, radius: 4.0, shadow: true, stripe: false, upper: false, font: "Merriweather" },
    KitDef { id: "retro", label: "Retro", about: "Black boxes, white edges, arcade capitals.", plate: Plate::Black, alpha: 1.0, outline: Outline::Light, outline_px: 2.0, radius: 0.0, shadow: false, stripe: false, upper: true, font: "Arcade" },
];

pub fn kit(id: &str) -> Option<&'static KitDef> {
    KITS.iter().find(|k| k.id == id)
}

// ============================================================================
// Styles
// ============================================================================

pub struct StyleDef {
    pub id: &'static str,
    pub label: &'static str,
    /// The Design tab's subtitle for the style.
    pub about: &'static str,
    pub kit: &'static str,
    pub palette: &'static str,
    pub heading_font: &'static str,
    pub body_font: &'static str,
    /// The brief's "Buttons" section.
    pub buttons: &'static str,
    /// The brief's "Panels, corners and decoration" section.
    pub panels: &'static str,
    /// The brief's "Click FX and feel" section.
    pub feel: &'static str,
    /// The brief's "Transitions" section.
    pub transitions: &'static str,
}

pub const STYLES: &[StyleDef] = &[
    StyleDef {
        id: "arena", label: "Arena", about: "Team colours, bold numbers, chunky plates",
        kit: "arcade", palette: "arena_night", heading_font: "GothamBlack", body_font: "GothamBold",
        buttons: "Solid plates in the primary colour with a dark outline; the one main action (Play, Find Match) is the biggest button on screen and the only green one.",
        panels: "Rounded plates, a hard shadow 4 px down, team colours at the top edge.",
        feel: "Every click lands with a short pop: the button dips and springs back.",
        transitions: "Screens rise from below and settle; the HUD never moves.",
    },
    StyleDef {
        id: "bubbly", label: "Bubbly", about: "Soft rounded shapes, pastel colours",
        kit: "chunky_toy", palette: "meadow", heading_font: "FredokaOne", body_font: "FredokaOne",
        buttons: "Fat pill buttons with a thick outline and a lighter top half.",
        panels: "Very round corners, cream plates, ink outlines.",
        feel: "Buttons squash on press and bounce back.",
        transitions: "Panels pop in from the centre.",
    },
    StyleDef {
        id: "adventure", label: "Adventure", about: "Parchment, brass and storybook frames",
        kit: "storybook", palette: "treasure", heading_font: "MerriweatherBold", body_font: "Merriweather",
        buttons: "Parchment tabs with ink edges; the main action is gold.",
        panels: "Paper plates, thin brown rules, small corner radius.",
        feel: "A soft page-turn on every open.",
        transitions: "Screens fade and slide a few pixels, like a page settling.",
    },
    StyleDef {
        id: "sleek", label: "Sleek", about: "Glass panels, thin lines, lots of air",
        kit: "glass", palette: "frost", heading_font: "GothamBold", body_font: "Gotham",
        buttons: "Glass pills; the main action is filled with the primary colour.",
        panels: "Frosted panes with a 1 px light edge and generous padding.",
        feel: "Hover brightens the edge; clicks are silent and quick.",
        transitions: "Short fades, nothing bounces.",
    },
    StyleDef {
        id: "candy", label: "Candy", about: "Saturated sweets, thick outlines",
        kit: "arcade", palette: "candy_pop", heading_font: "FredokaOne", body_font: "GothamBold",
        buttons: "Candy-coloured plates, thick ink outlines, big type.",
        panels: "Round corners, hard shadows, a different colour per section.",
        feel: "Pops and wobbles; rewards burst.",
        transitions: "Panels drop in and bounce once.",
    },
    StyleDef {
        id: "tactical", label: "Tactical", about: "Military: sharp corners, amber on slate",
        kit: "tactical", palette: "field_ops", heading_font: "GothamBold", body_font: "RobotoMono",
        buttons: "Square outlined buttons, capitals, the accent for the one live action.",
        panels: "Square corners, 1 px lines, slate at 70 percent.",
        feel: "A crisp tick on click, no bounce.",
        transitions: "Instant, or a 100 ms wipe.",
    },
    StyleDef {
        id: "horror", label: "Horror", about: "Minimal, typewriter, blood",
        kit: "minimal", palette: "blood_moon", heading_font: "MerriweatherBold", body_font: "SpecialElite",
        buttons: "Plain words that glow on hover; no plates.",
        panels: "Almost none: text on darkness, one thin rule.",
        feel: "Clicks are quiet; the screen darkens a little on open.",
        transitions: "Slow fades from black.",
    },
];

pub fn style(id: &str) -> Option<&'static StyleDef> {
    STYLES.iter().find(|s| s.id == id)
}

// ============================================================================
// Genres and examples
// ============================================================================

pub struct GenreDef {
    pub id: &'static str,
    pub label: &'static str,
    /// Words that point at this genre in a prompt, lowercase.
    pub keywords: &'static [&'static str],
    pub style: &'static str,
    /// Title parts; the generator picks one of each by seed.
    pub title_a: &'static [&'static str],
    pub title_b: &'static [&'static str],
    pub tagline: &'static str,
    /// The Examples menu: its line and the prompt it fills in.
    pub example_title: &'static str,
    pub example: &'static str,
    /// The icon file (under `assets/icons/ui/`) the Examples menu shows.
    pub icon: &'static str,
}

pub const GENRES: &[GenreDef] = &[
    GenreDef {
        id: "tycoon", label: "Tycoon",
        keywords: &["tycoon", "factory", "business", "idle", "upgrade", "dropper", "conveyor", "empire"],
        style: "bubbly",
        title_a: &["Factory", "Candy", "Mega", "Tiny", "Golden"], title_b: &["Tycoon", "Empire", "Works", "Inc."],
        tagline: "Build it up, cash it in.",
        example_title: "Tycoon",
        example: "A cheerful factory tycoon: cash counter, upgrade shop, rebirth button and a daily reward.",
        icon: "factory",
    },
    GenreDef {
        id: "rpg", label: "RPG",
        keywords: &["rpg", "quest", "fantasy", "dungeon", "sword", "magic", "level", "inventory", "adventure"],
        style: "adventure",
        title_a: &["Treasure", "Ember", "Crown", "Moon"], title_b: &["Quest", "Realms", "Legends", "Keep"],
        tagline: "Every road ends in treasure.",
        example_title: "RPG",
        example: "A fantasy adventure RPG like Treasure Quest: gold, health and mana bars, quest log, inventory and a shop.",
        icon: "book",
    },
    GenreDef {
        id: "obby", label: "Obby",
        keywords: &["obby", "parkour", "obstacle", "stage", "checkpoint", "tower of", "jump"],
        style: "candy",
        title_a: &["Mega", "Sky", "Rainbow", "Impossible"], title_b: &["Obby", "Climb", "Tower", "Run"],
        tagline: "One more stage.",
        example_title: "Obby",
        example: "A bright obby: stage counter, timer, skip-stage button, trails shop and a checkpoint banner.",
        icon: "flag",
    },
    GenreDef {
        id: "racing", label: "Racing",
        keywords: &["racing", "race", "car", "kart", "drift", "lap", "speed", "track"],
        style: "arena",
        title_a: &["Turbo", "Nitro", "Drift", "Apex"], title_b: &["Rush", "Kings", "Circuit", "Legends"],
        tagline: "First to the line.",
        example_title: "Racing",
        example: "An arcade racing game: speedometer, lap counter, race position, nitro meter and a garage.",
        icon: "vehicle",
    },
    GenreDef {
        id: "tower_defense", label: "Tower Defense",
        keywords: &["tower defense", "tower defence", "td", "wave", "towers", "defend", "lane"],
        style: "tactical",
        title_a: &["Tower", "Last", "Iron", "Castle"], title_b: &["Siege", "Stand", "Line", "Watch"],
        tagline: "Hold the line.",
        example_title: "Tower Defense",
        example: "A tower defense game: wave counter, base health, cash, a tower hotbar and a units shop.",
        icon: "shield",
    },
    GenreDef {
        id: "battle_royale", label: "Battle Royale",
        keywords: &["battle royale", "royale", "last one", "storm", "drop", "squad", "loot"],
        style: "arena",
        title_a: &["Last", "Omega", "Storm", "Final"], title_b: &["Drop", "Zone", "Circle", "Front"],
        tagline: "Drop in. Last one out.",
        example_title: "Battle Royale",
        example: "A cartoony battle royale: players left, storm timer, minimap, ammo hotbar, emote wheel and a season pass.",
        icon: "crosshair",
    },
    GenreDef {
        id: "horror", label: "Horror",
        keywords: &["horror", "scary", "creepy", "doors", "escape", "haunted", "monster", "fear", "flashlight"],
        style: "horror",
        title_a: &["The Silent", "The Hollow", "The Last", "Blackwood"], title_b: &["Inn", "House", "Floor", "Hall"],
        tagline: "Every hundredth door is different.",
        example_title: "Horror",
        example: "A tense horror escape game like Doors: minimal typewriter HUD, flashlight battery, stamina, objective text, a fear vignette and a creepy main menu.",
        icon: "flashlight",
    },
    GenreDef {
        id: "roleplay", label: "Roleplay",
        keywords: &["roleplay", "rp", "town", "city", "life", "house", "job", "brookhaven", "family"],
        style: "sleek",
        title_a: &["Welcome to", "Maple", "Sunny", "Harbor"], title_b: &["Maple", "Town", "Bay", "Life"],
        tagline: "Live your story.",
        example_title: "Roleplay",
        example: "A cozy town roleplay: money, job title, phone menu, house shop, vehicle spawner and outfits.",
        icon: "users",
    },
    GenreDef {
        id: "fps", label: "Shooter",
        keywords: &["fps", "shooter", "gun", "arsenal", "rivals", "team deathmatch", "ammo", "sniper", "weapon"],
        style: "arena",
        title_a: &["Omega", "Viper", "Frontline", "Rogue"], title_b: &["Front", "Strike", "Squad", "Ops"],
        tagline: "First to fifty.",
        example_title: "Shooter",
        example: "A team shooter like Rivals: team score, round timer, health, ammo, a weapon hotbar, kill feed and a crate shop.",
        icon: "crosshair",
    },
    GenreDef {
        id: "simulator", label: "Simulator",
        keywords: &["simulator", "sim", "clicker", "pet", "collect", "rebirth", "egg", "farm", "mining"],
        style: "bubbly",
        title_a: &["Pet", "Mining", "Bubble", "Speed"], title_b: &["Simulator", "Legends", "World", "Kingdom"],
        tagline: "Collect them all.",
        example_title: "Simulator",
        example: "A pet simulator: coins and gems, backpack capacity, rebirths, egg shop, pet inventory and boosts.",
        icon: "sparkle",
    },
    GenreDef {
        id: "survival", label: "Survival",
        keywords: &["survival", "survive", "hunger", "thirst", "craft", "island", "zombie", "night"],
        style: "tactical",
        title_a: &["Dead", "Wild", "Last", "Lost"], title_b: &["Island", "Winter", "Night", "Signal"],
        tagline: "Make it to morning.",
        example_title: "Survival",
        example: "A survival game: health, hunger and thirst bars, day counter, crafting menu and a backpack hotbar.",
        icon: "fire",
    },
];

pub fn genre(id: &str) -> Option<&'static GenreDef> {
    GENRES.iter().find(|g| g.id == id)
}

/// The genre a prompt is about: most keyword hits wins, ties go to the
/// earlier genre, and a prompt that names none is a simulator.
pub fn detect_genre(prompt: &str) -> &'static GenreDef {
    let text = prompt.to_lowercase();
    let mut best: Option<(&'static GenreDef, usize)> = None;
    for g in GENRES {
        let hits = g.keywords.iter().filter(|k| contains_word(&text, k)).count();
        if hits > 0 && best.map_or(true, |(_, h)| hits > h) {
            best = Some((g, hits));
        }
    }
    best.map(|(g, _)| g).or_else(|| genre("simulator")).unwrap_or(&GENRES[0])
}

/// Whether `needle` appears in `haystack` as whole words ("rp" must not
/// match "sharp").
pub fn contains_word(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(needle) {
        let at = start + pos;
        let end = at + needle.len();
        let before_ok = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        let after_ok = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric()
            // "towers", "quests": a plural still counts.
            || (bytes[end] == b's' && (end + 1 >= bytes.len() || !bytes[end + 1].is_ascii_alphanumeric()));
        if before_ok && after_ok {
            return true;
        }
        start = at + 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_names_a_real_kit_and_palette() {
        for s in STYLES {
            assert!(kit(s.kit).is_some(), "{} kit {}", s.id, s.kit);
            assert!(palette(s.palette).is_some(), "{} palette {}", s.id, s.palette);
        }
        for g in GENRES {
            assert!(style(g.style).is_some(), "{} style {}", g.id, g.style);
        }
    }

    #[test]
    fn genres_are_found_by_their_words() {
        assert_eq!(detect_genre("A tense horror escape game like Doors").id, "horror");
        assert_eq!(detect_genre("a cartoony battle royale with a minimap").id, "battle_royale");
        assert_eq!(detect_genre("my cool tycoon with upgrades").id, "tycoon");
        assert_eq!(detect_genre("something").id, "simulator");
        // "rp" is a word, not part of "sharp".
        assert_ne!(detect_genre("a sharp sword").id, "roleplay");
    }

    #[test]
    fn whole_words_only() {
        assert!(contains_word("build towers here", "tower"));
        assert!(!contains_word("sharp", "rp"));
        assert!(contains_word("rp", "rp"));
    }
}
