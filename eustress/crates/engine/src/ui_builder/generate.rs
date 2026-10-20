//! The offline generator: one line of prompt to a whole [`Blueprint`], with
//! no network and no API key.
//!
//! 1. The genre comes from the prompt's words ([`catalog::detect_genre`]),
//!    and gives the base HUD and screens.
//! 2. Features the prompt names ("stamina", "minimap", "emotes", "codes")
//!    add or reshape elements and screens on top.
//! 3. A style word in the prompt ("sleek", "tactical") overrides the
//!    genre's style; the style gives the kit, palette and fonts.
//! 4. The seed picks the title and, for a Variation, a different palette
//!    and kit from the ones that suit the style.
//!
//! The same prompt and seed always give the same blueprint, so Regenerate
//! is repeatable and Variation is the only thing that changes the result.

use super::blueprint::*;
use super::catalog::{self, contains_word, GenreDef, StyleDef};

/// A stable 64-bit hash of the prompt (FNV-1a), the first seed.
pub fn seed_for(prompt: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in prompt.trim().to_lowercase().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // Four digits read better in "seed 4121" than twenty.
    h % 9000 + 1000
}

/// Pick one of `options` by seed and a salt, so different picks from one
/// seed do not all land on the same index.
fn pick<'a, T>(options: &'a [T], seed: u64, salt: u64) -> &'a T {
    let mixed = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(salt.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    &options[(mixed >> 7) as usize % options.len()]
}

/// The whole blueprint for a prompt.
pub fn generate(prompt: &str, seed: u64) -> Blueprint {
    let genre = catalog::detect_genre(prompt);
    let style = style_for(prompt, genre);
    let text = prompt.to_lowercase();

    let title = title_for(prompt, genre, seed);
    let mut bp = Blueprint {
        version: BLUEPRINT_VERSION,
        name: pascal_name(&title),
        title: title.clone(),
        tagline: genre.tagline.to_string(),
        prompt: prompt.trim().to_string(),
        genre: genre.id.to_string(),
        style: style.id.to_string(),
        kit: "auto".to_string(),
        palette: catalog::palette(style.palette).map(|p| p.to_palette()).unwrap_or_default(),
        heading_font: style.heading_font.to_string(),
        body_font: style.body_font.to_string(),
        seed,
        source: "offline".to_string(),
        hud: Vec::new(),
        screens: Vec::new(),
    };

    // A Variation (any seed other than the prompt's own) tries a different
    // palette and kit that still suit the style.
    if seed != seed_for(prompt) {
        let palettes = palettes_for(style.id);
        if let Some(p) = catalog::palette(*pick(palettes, seed, 3)) {
            bp.palette = p.to_palette();
        }
        bp.kit = pick(kits_for(style.id), seed, 5).to_string();
    }

    base_for(genre.id, &mut bp);
    apply_features(&text, &mut bp);
    tidy(&mut bp);
    bp
}

/// The style a prompt asks for by name, else its genre's.
fn style_for(prompt: &str, genre: &GenreDef) -> &'static StyleDef {
    let text = prompt.to_lowercase();
    for s in catalog::STYLES {
        if contains_word(&text, &s.label.to_lowercase()) {
            return s;
        }
    }
    // Words that point at a style without naming it.
    let hints: &[(&str, &str)] = &[
        ("cartoony", "bubbly"), ("cute", "bubbly"), ("cozy", "bubbly"), ("pastel", "bubbly"),
        ("military", "tactical"), ("realistic", "tactical"), ("gritty", "tactical"),
        ("minimal", "sleek"), ("clean", "sleek"), ("modern", "sleek"), ("futuristic", "sleek"),
        ("fantasy", "adventure"), ("medieval", "adventure"), ("pirate", "adventure"),
        ("creepy", "horror"), ("scary", "horror"), ("spooky", "horror"),
        ("colorful", "candy"), ("colourful", "candy"), ("sweet", "candy"),
        ("competitive", "arena"), ("esports", "arena"),
    ];
    for (word, id) in hints {
        if contains_word(&text, word) {
            if let Some(s) = catalog::style(id) {
                // "cartoony battle royale" stays arena-bold rather than
                // turning a shooter into a toy box; only horror and
                // adventure words override a genre that has its own look.
                if *id == "bubbly" && matches!(genre.id, "fps" | "battle_royale") {
                    continue;
                }
                return s;
            }
        }
    }
    catalog::style(genre.style).unwrap_or(&catalog::STYLES[0])
}

/// Palettes that suit a style, its own first.
fn palettes_for(style: &str) -> &'static [&'static str] {
    match style {
        "arena" => &["arena_night", "neon_pulse", "ocean", "sunset"],
        "bubbly" => &["meadow", "candy_pop", "sunset", "ocean"],
        "adventure" => &["treasure", "hotel_brass", "meadow", "sunset"],
        "sleek" => &["frost", "mono", "ocean", "neon_pulse"],
        "candy" => &["candy_pop", "sunset", "neon_pulse", "meadow"],
        "tactical" => &["field_ops", "mono", "frost", "blood_moon"],
        "horror" => &["blood_moon", "hotel_brass", "mono", "field_ops"],
        _ => &["arena_night"],
    }
}

/// Kits that suit a style, its own first.
fn kits_for(style: &str) -> &'static [&'static str] {
    match style {
        "arena" => &["arcade", "competitive", "neon", "original"],
        "bubbly" => &["chunky_toy", "glass", "arcade"],
        "adventure" => &["storybook", "original", "chunky_toy"],
        "sleek" => &["glass", "minimal", "original"],
        "candy" => &["arcade", "chunky_toy", "neon"],
        "tactical" => &["tactical", "competitive", "retro"],
        "horror" => &["minimal", "storybook", "original", "tactical"],
        _ => &["original"],
    }
}

/// A title the prompt names ("called Blackwood", "named 'Omega Front'",
/// or anything in quotes), else two parts picked by seed.
fn title_for(prompt: &str, genre: &GenreDef, seed: u64) -> String {
    if let Some(quoted) = quoted_title(prompt) {
        return quoted;
    }
    // ASCII lowercasing keeps every byte offset, so `at` indexes `prompt`.
    let lower = prompt.to_ascii_lowercase();
    for marker in ["called ", "named ", "titled "] {
        if let Some(at) = lower.find(marker) {
            let rest = &prompt[at + marker.len()..];
            let words: Vec<&str> = rest
                .split(|c: char| c == ',' || c == '.' || c == ':' || c == ';' || c == '\n')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .take(4)
                .collect();
            if !words.is_empty() {
                return words.join(" ");
            }
        }
    }
    let a = pick(genre.title_a, seed, 1);
    let mut b = *pick(genre.title_b, seed, 2);
    // "Welcome to" + "Maple" but never "Maple Maple".
    if a.ends_with(b) {
        b = genre.title_b.iter().copied().find(|x| !a.ends_with(x)).unwrap_or(b);
    }
    format!("{a} {b}")
}

fn quoted_title(prompt: &str) -> Option<String> {
    // Double quotes only: single quotes are apostrophes in "don't" and
    // "it's", and would quote the words between them.
    for (open, close) in [('"', '"'), ('\u{201c}', '\u{201d}')] {
        if let Some(start) = prompt.find(open) {
            let rest = &prompt[start + open.len_utf8()..];
            if let Some(end) = rest.find(close) {
                let t = rest[..end].trim();
                if (2..=32).contains(&t.len()) && t.chars().any(|c| c.is_alphabetic()) {
                    return Some(t.to_string());
                }
            }
        }
    }
    None
}

// ============================================================================
// Shared screens
// ============================================================================

fn loading(bp: &Blueprint) -> Screen {
    Screen::new("loading", "Loading", ScreenKind::Loading).items(vec![Item::new(&bp.tagline, "", "")])
}

fn settings() -> Screen {
    Screen::new("settings", "Settings", ScreenKind::Settings)
        .tabs(["General", "Graphics", "Audio", "Controls"])
        .items(vec![
            Item::new("Music", "Background music", "on"),
            Item::new("Sound effects", "Clicks, hits and pickups", "on"),
            Item::new("Camera shake", "Shake on hits and explosions", "on"),
            Item::new("Show FPS", "Frame rate in the corner", "off"),
        ])
}

fn codes() -> Screen {
    Screen::new("codes", "Codes", ScreenKind::Codes)
        .items(vec![Item::new("Enter a code", "Codes come from updates and the community", "")])
}

fn results(title: &str, rows: &[(&str, &str)]) -> Screen {
    Screen::new("results", title, ScreenKind::Results)
        .items(rows.iter().map(|(n, v)| Item::new(n, "", v)).collect())
}

fn list(id: &str, title: &str, rows: &[(&str, &str, &str)]) -> Screen {
    Screen::new(id, title, ScreenKind::List)
        .items(rows.iter().map(|(n, d, v)| Item::new(n, d, v)).collect())
}

fn grid(id: &str, title: &str, names: &[&str]) -> Screen {
    Screen::new(id, title, ScreenKind::Grid)
        .items(names.iter().map(|n| Item::new(n, "", "")).collect())
}

fn shop(id: &str, title: &str, tabs: &[&str], rows: &[(&str, &str, &str)]) -> Screen {
    Screen::new(id, title, ScreenKind::Shop)
        .tabs(tabs.iter().copied())
        .items(rows.iter().map(|(n, d, v)| Item::new(n, d, v)).collect())
}

fn pass(tiers: &[&str]) -> Screen {
    Screen::new("pass", "Season Pass", ScreenKind::Pass)
        .items(tiers.iter().enumerate().map(|(i, n)| Item::new(n, "", &format!("{}", i + 1))).collect())
}

fn main_menu(id: &str, title: &str, buttons: &[(&str, &str)], offer: &str) -> Screen {
    let mut s = Screen::new(id, title, ScreenKind::MainMenu)
        .items(buttons.iter().map(|(label, opens)| Item::new(label, "", "").opens(opens)).collect());
    if !offer.is_empty() {
        s.tabs = vec![offer.to_string()];
    }
    s
}

// ============================================================================
// Genres
// ============================================================================

/// The HUD and screens every game of the genre starts from.
fn base_for(genre: &str, bp: &mut Blueprint) {
    use Anchor::*;
    use ElementKind as K;
    let load = loading(bp);
    match genre {
        "horror" => {
            bp.hud = vec![
                Element::new(K::Counter, "hud_counter", "ROOM").at(TopCenter).value("79").digits(4),
                Element::new(K::Objective, "hud_objective", "GUIDING LIGHT").at(TopRight).value("Find the key to door 80"),
                Element::new(K::Currency, "hud_wallet", "Knobs").at(TopLeft).value("200").icon("$"),
                Element::new(K::Button, "hud_journal", "Journal").at(TopLeft).opens("notes"),
                Element::new(K::Tracker, "hud_quests", "Quests").at(TopLeft)
                    .items(["Collect 20 batteries|20/20", "Read 15 journal notes|2/15"]),
                Element::new(K::Meter, "hud_battery", "Battery").at(BottomLeft).value("80").max("100").icon("!"),
                Element::new(K::Hotbar, "hud_hotbar", "Lighter").at(BottomCenter).value("1")
                    .items(["Lighter", "Crucifix", "Vitamins", "Lockpick", "Key"]),
                Element::new(K::Controls, "hud_controls", "Controls").at(BottomRight)
                    .items(["Q|Journal", "E|Hide", "C|Crouch", "Shift|Sprint"]),
            ];
            bp.screens = vec![
                load,
                main_menu("main_menu", &bp.title, &[("Play", ""), ("Pre-Run Shop", "pre_run_shop"), ("Codes", "codes"), ("Settings", "settings")], ""),
                shop("pre_run_shop", "Pre-Run Shop", &["Items", "Boosts"], &[
                    ("Flashlight", "Brighter, longer battery", "60"),
                    ("Lockpick", "Opens one locked door", "40"),
                    ("Vitamins", "A burst of speed", "25"),
                    ("Crucifix", "Stops one monster", "200"),
                ]),
                Screen::new("notes", "Journal", ScreenKind::Info).items(vec![
                    Item::new("Night 1", "The elevator only goes down. The desk clerk never looks up.", ""),
                    Item::new("Night 2", "Room 50 has no door. I heard someone inside anyway.", ""),
                    Item::new("Night 3", "Keep the light on. Whatever you do, keep the light on.", ""),
                ]),
                list("missions", "Missions", &[
                    ("Collect 20 batteries", "Reward: 50 knobs", "20/20"),
                    ("Read 15 journal notes", "Reward: 100 knobs", "2/15"),
                    ("Reach door 100", "Reward: Crucifix", "79/100"),
                ]),
                codes(),
                settings(),
                results("Lights Out", &[("Doors opened", "79"), ("Knobs found", "212"), ("Time", "18:42")]),
            ];
        }
        "battle_royale" => {
            bp.hud = vec![
                Element::new(K::Timer, "hud_storm", "STORM CLOSES").at(TopCenter).value("3:59"),
                Element::new(K::Counter, "hud_alive", "ALIVE").at(TopCenter).value("42"),
                Element::new(K::Minimap, "hud_minimap", "Map").at(TopRight),
                Element::new(K::Currency, "hud_coins", "Coins").at(TopRight).value("2,500").icon("$"),
                Element::new(K::Button, "hud_menu", "Menu").at(TopLeft).opens("lobby"),
                Element::new(K::Menu, "hud_side", "").at(Left)
                    .link("Challenges", "challenges", "3").link("Season Pass", "pass", "1"),
                Element::new(K::Meter, "hud_health", "Health").at(BottomLeft).value("100").max("100").icon("+"),
                Element::new(K::Meter, "hud_shield", "Shield").at(BottomLeft).value("50").max("100").icon("#"),
                Element::new(K::Hotbar, "hud_hotbar", "Viper AR").at(BottomCenter).value("1")
                    .items(["Viper AR", "Shotgun", "Medkit", "Grenade", "Build"]),
                Element::new(K::Controls, "hud_controls", "Controls").at(BottomRight)
                    .items(["Tab|Scoreboard", "R|Reload", "C|Crouch", "Shift|Sprint"]),
            ];
            bp.screens = vec![
                load,
                main_menu("lobby", "Lobby", &[("Find Match", ""), ("Party", "party"), ("Locker", "locker"), ("Item Shop", "item_shop"), ("Settings", "settings")], "Rookie Pack"),
                grid("party", "Party", &["You", "Invite", "Invite", "Invite"]),
                grid("locker", "Locker", &["Outfit", "Back Bling", "Pickaxe", "Glider", "Wrap", "Emote"]),
                shop("item_shop", "Item Shop", &["Featured", "Daily", "Bundles"], &[
                    ("Mythic Bundle", "7,250 tokens and a title", "1,499"),
                    ("Galaxy Glider", "Leaves a star trail", "1,500"),
                    ("Newcomer Bundle", "725 tokens and a wrap", "149"),
                    ("Neon Pickaxe", "Glows at night", "800"),
                ]),
                list("challenges", "Challenges", &[
                    ("Land at the Docks", "5,000 XP", "0/1"),
                    ("Deal 500 damage", "8,000 XP", "320/500"),
                    ("Finish top 10 three times", "12,000 XP", "1/3"),
                ]),
                pass(&["Spray", "100 Coins", "Emote", "Wrap", "Glider", "Outfit"]),
                grid("emotes", "Emotes", &["Wave", "Dance", "Salute", "Laugh", "Floss", "Sit"]),
                results("Victory", &[("Placement", "#1"), ("Eliminations", "7"), ("Damage", "1,240"), ("XP earned", "18,500")]),
                settings(),
            ];
        }
        "fps" => {
            bp.hud = vec![
                Element::new(K::TeamScore, "hud_score", "FIRST TO 50").at(TopCenter).value("39-45")
                    .items(["Blue Team", "Red Team"]),
                Element::new(K::Timer, "hud_round", "ROUND ENDS").at(TopCenter).value("3:13"),
                Element::new(K::Minimap, "hud_minimap", "Map").at(TopRight),
                Element::new(K::Currency, "hud_coins", "Coins").at(TopRight).value("2,500").icon("$"),
                Element::new(K::Currency, "hud_stars", "Stars").at(TopRight).value("300").icon("*"),
                Element::new(K::Button, "hud_menu", "Menu").at(TopLeft).opens("lobby"),
                Element::new(K::Menu, "hud_side", "").at(Left)
                    .link("Missions", "missions", "1").link("Pass", "pass", "2"),
                Element::new(K::Meter, "hud_health", "Health").at(BottomLeft).value("100").max("100").icon("+"),
                Element::new(K::Hotbar, "hud_hotbar", "Viper AR").at(BottomCenter).value("1")
                    .items(["Viper AR", "Pistol", "Knife", "Grenade"]),
                Element::new(K::Controls, "hud_controls", "Controls").at(BottomRight)
                    .items(["Tab|Scoreboard", "R|Reload", "C|Crouch", "Shift|Sprint"]),
            ];
            bp.screens = vec![
                load,
                main_menu("lobby", "Lobby", &[("Find Match", ""), ("Party", "party"), ("Loadout", "loadout"), ("Inventory", "inventory"), ("Store", "store"), ("Settings", "settings")], "Rookie Pack"),
                grid("party", "Party", &["You", "Invite", "Invite", "Invite"]),
                grid("loadout", "Loadout", &["Primary", "Secondary", "Melee", "Utility"]),
                grid("inventory", "Inventory", &["Viper AR", "Pistol", "Knife", "Grenade", "Smoke", "Medkit"]),
                shop("store", "Store", &["Featured", "Cases", "Skins", "Tokens", "Passes"], &[
                    ("Mythic Bundle", "7,250 tokens and a title", "1,499"),
                    ("Galaxy Skin", "For a limited time", "1,500"),
                    ("Newcomer Bundle", "725 tokens and a case", "149"),
                    ("Weapon Case", "One random skin", "250"),
                ]),
                list("missions", "Missions", &[
                    ("Win 3 matches", "500 coins", "1/3"),
                    ("Get 25 eliminations", "250 coins", "18/25"),
                    ("Capture 5 objectives", "100 stars", "2/5"),
                ]),
                pass(&["Sticker", "250 Coins", "Charm", "Skin", "Case", "Knife Skin"]),
                Screen::new("scoreboard", "Scoreboard", ScreenKind::Leaderboard).items(vec![
                    Item::new("Viper", "Blue Team", "24"), Item::new("Nova", "Red Team", "21"),
                    Item::new("Ghost", "Blue Team", "17"), Item::new("Rook", "Red Team", "12"),
                ]),
                results("Victory", &[("Eliminations", "24"), ("Deaths", "9"), ("Accuracy", "41%"), ("Coins earned", "350")]),
                settings(),
            ];
        }
        "tycoon" => {
            bp.hud = vec![
                Element::new(K::Currency, "hud_cash", "Cash").at(TopRight).value("$1,250").icon("$"),
                Element::new(K::Currency, "hud_gems", "Gems").at(TopRight).value("40").icon("*"),
                Element::new(K::Counter, "hud_rebirths", "REBIRTHS").at(TopCenter).value("2"),
                Element::new(K::Menu, "hud_side", "").at(Left)
                    .link("Shop", "shop", "").link("Rebirth", "rebirth", "").link("Daily", "daily", "1").link("Codes", "codes", ""),
                Element::new(K::Meter, "hud_upgrade", "Next upgrade").at(BottomCenter).value("64").max("100").icon("^"),
                Element::new(K::Button, "hud_settings", "Settings").at(BottomLeft).opens("settings"),
            ];
            bp.screens = vec![
                load,
                shop("shop", "Shop", &["Droppers", "Upgrades", "Boosts"], &[
                    ("Basic Dropper", "+$5 a second", "$250"),
                    ("Conveyor Speed", "Items move 25% faster", "$1,000"),
                    ("Golden Dropper", "+$50 a second", "$12,500"),
                    ("2x Cash", "Doubles every sale", "R$ 199"),
                ]),
                Screen::new("rebirth", "Rebirth", ScreenKind::Results).items(vec![
                    Item::new("Next multiplier", "", "x3"), Item::new("Cost", "", "$1,000,000"), Item::new("You keep", "", "Gems and pets"),
                ]),
                grid("daily", "Daily Rewards", &["Day 1", "Day 2", "Day 3", "Day 4", "Day 5", "Day 6", "Day 7"]),
                codes(),
                settings(),
            ];
        }
        "rpg" => {
            bp.hud = vec![
                Element::new(K::Currency, "hud_gold", "Gold").at(TopRight).value("1,240").icon("$"),
                Element::new(K::Meter, "hud_health", "Health").at(BottomLeft).value("86").max("100").icon("+"),
                Element::new(K::Meter, "hud_mana", "Mana").at(BottomLeft).value("40").max("60").icon("*"),
                Element::new(K::Counter, "hud_level", "LEVEL").at(TopLeft).value("12"),
                Element::new(K::Objective, "hud_objective", "QUEST").at(TopCenter).value("Find the moonstone in the old mine"),
                Element::new(K::Minimap, "hud_minimap", "Map").at(TopRight),
                Element::new(K::Hotbar, "hud_hotbar", "Skills").at(BottomCenter).value("1")
                    .items(["Slash", "Fireball", "Heal", "Dash", "Potion"]),
                Element::new(K::Menu, "hud_side", "").at(Right)
                    .link("Inventory", "inventory", "").link("Quests", "quests", "2").link("Shop", "shop", ""),
            ];
            bp.screens = vec![
                load,
                main_menu("main_menu", &bp.title, &[("Continue", ""), ("New Game", ""), ("Settings", "settings")], ""),
                grid("inventory", "Inventory", &["Iron Sword", "Oak Shield", "Health Potion", "Mana Potion", "Moonstone", "Map", "Key", "Bread"]),
                list("quests", "Quest Log", &[
                    ("The Moonstone", "Find it in the old mine", "0/1"),
                    ("Wolf Trouble", "Hunt 8 wolves", "5/8"),
                    ("Herbalist", "Pick 10 silverleaf", "10/10"),
                ]),
                shop("shop", "Merchant", &["Weapons", "Armor", "Potions"], &[
                    ("Steel Sword", "+12 attack", "450"),
                    ("Chain Mail", "+20 defense", "600"),
                    ("Health Potion", "Restores 50 health", "35"),
                    ("Mana Potion", "Restores 30 mana", "40"),
                ]),
                settings(),
            ];
        }
        "obby" => {
            bp.hud = vec![
                Element::new(K::Counter, "hud_stage", "STAGE").at(TopCenter).value("27").max("100"),
                Element::new(K::Timer, "hud_timer", "TIME").at(TopCenter).value("04:12"),
                Element::new(K::Currency, "hud_coins", "Coins").at(TopRight).value("310").icon("$"),
                Element::new(K::Menu, "hud_side", "").at(Left)
                    .link("Skip Stage", "", "").link("Trails", "trails", "").link("Codes", "codes", ""),
                Element::new(K::Banner, "hud_banner", "CHECKPOINT").at(TopCenter).value("Stage 27 saved"),
            ];
            bp.screens = vec![
                load,
                shop("trails", "Trails", &["Trails", "Gear"], &[
                    ("Rainbow Trail", "Leaves a rainbow", "250"),
                    ("Fire Trail", "Leaves flames", "400"),
                    ("Speed Coil", "Run 25% faster", "R$ 99"),
                    ("Gravity Coil", "Jump higher", "R$ 99"),
                ]),
                codes(),
                settings(),
                results("Tower Complete", &[("Time", "14:02"), ("Deaths", "63"), ("Coins", "+500")]),
            ];
        }
        "racing" => {
            bp.hud = vec![
                Element::new(K::Counter, "hud_position", "POS").at(TopLeft).value("3").max("8"),
                Element::new(K::Counter, "hud_lap", "LAP").at(TopCenter).value("2").max("3"),
                Element::new(K::Timer, "hud_time", "TIME").at(TopRight).value("1:24.6"),
                Element::new(K::Minimap, "hud_minimap", "Track").at(TopRight),
                Element::new(K::Counter, "hud_speed", "KM/H").at(BottomRight).value("214"),
                Element::new(K::Meter, "hud_nitro", "Nitro").at(BottomRight).value("70").max("100").icon(">"),
                Element::new(K::Button, "hud_menu", "Menu").at(TopLeft).opens("garage"),
            ];
            bp.screens = vec![
                load,
                main_menu("main_menu", &bp.title, &[("Race", ""), ("Garage", "garage"), ("Dealership", "dealership"), ("Settings", "settings")], "Starter Pack"),
                grid("garage", "Garage", &["Street", "Drift", "Muscle", "Rally", "Super", "Kart"]),
                shop("dealership", "Dealership", &["Cars", "Parts", "Paint"], &[
                    ("Drift King", "Loose rear, big smoke", "12,000"),
                    ("Turbo Kit", "+15% top speed", "3,500"),
                    ("Neon Underglow", "Pick any colour", "900"),
                    ("VIP Garage", "Four more slots", "R$ 249"),
                ]),
                results("Race Over", &[("Position", "1st"), ("Best lap", "0:41.2"), ("Cash", "+1,200")]),
                settings(),
            ];
        }
        "tower_defense" => {
            bp.hud = vec![
                Element::new(K::Counter, "hud_wave", "WAVE").at(TopCenter).value("12").max("40"),
                Element::new(K::Meter, "hud_base", "Base").at(TopLeft).value("850").max("1000").icon("+"),
                Element::new(K::Currency, "hud_cash", "Cash").at(TopRight).value("$3,420").icon("$"),
                Element::new(K::Timer, "hud_next", "NEXT WAVE").at(TopCenter).value("0:12"),
                Element::new(K::Hotbar, "hud_towers", "Towers").at(BottomCenter).value("1")
                    .items(["Scout", "Sniper", "Minigun", "Farm", "Commander"]),
                Element::new(K::Menu, "hud_side", "").at(Left)
                    .link("Units", "units", "").link("Skip Wave", "", "").link("Settings", "settings", ""),
            ];
            bp.screens = vec![
                load,
                main_menu("lobby", "Lobby", &[("Play", ""), ("Units", "units"), ("Crates", "crates"), ("Settings", "settings")], ""),
                grid("units", "Units", &["Scout", "Sniper", "Minigun", "Farm", "Commander", "Empty", "Empty", "Empty"]),
                shop("crates", "Crates", &["Crates", "Gems"], &[
                    ("Normal Crate", "Common to rare", "500"),
                    ("Elite Crate", "Rare to legendary", "2,000"),
                    ("Golden Crate", "Epic or better", "R$ 149"),
                    ("Gem Pack", "1,000 gems", "R$ 99"),
                ]),
                results("Victory", &[("Waves", "40/40"), ("Towers placed", "18"), ("Coins", "+640")]),
                settings(),
            ];
        }
        "roleplay" => {
            bp.hud = vec![
                Element::new(K::Currency, "hud_money", "Money").at(TopRight).value("$4,800").icon("$"),
                Element::new(K::Objective, "hud_job", "JOB").at(TopLeft).value("Barista at Maple Cafe"),
                Element::new(K::Menu, "hud_side", "").at(Left)
                    .link("Phone", "phone", "").link("Houses", "houses", "").link("Vehicles", "vehicles", "").link("Outfits", "outfits", ""),
                Element::new(K::Timer, "hud_clock", "DAY 3").at(TopCenter).value("08:30"),
            ];
            bp.screens = vec![
                load,
                main_menu("main_menu", &format!("Welcome to {}", bp.title.trim_start_matches("Welcome to ").trim()), &[("Play", ""), ("Outfits", "outfits"), ("Settings", "settings")], ""),
                grid("phone", "Phone", &["Messages", "Jobs", "Bank", "Map", "Camera", "Music"]),
                shop("houses", "Houses", &["Starter", "Family", "Mansion"], &[
                    ("Maple Cottage", "2 rooms, garden", "Free"),
                    ("Lake House", "4 rooms, dock", "$25,000"),
                    ("Hill Mansion", "9 rooms, pool", "R$ 299"),
                    ("Tree House", "1 room, great view", "$8,000"),
                ]),
                grid("vehicles", "Vehicles", &["Bike", "Scooter", "Hatchback", "Pickup", "Bus", "Boat"]),
                grid("outfits", "Outfits", &["Casual", "Work", "Sport", "Formal", "Pyjamas", "Costume"]),
                settings(),
            ];
        }
        "survival" => {
            bp.hud = vec![
                Element::new(K::Meter, "hud_health", "Health").at(BottomLeft).value("90").max("100").icon("+"),
                Element::new(K::Meter, "hud_hunger", "Hunger").at(BottomLeft).value("60").max("100").icon("o"),
                Element::new(K::Meter, "hud_thirst", "Thirst").at(BottomLeft).value("45").max("100").icon("~"),
                Element::new(K::Counter, "hud_day", "DAY").at(TopCenter).value("7"),
                Element::new(K::Timer, "hud_night", "NIGHT IN").at(TopCenter).value("2:40"),
                Element::new(K::Hotbar, "hud_hotbar", "Axe").at(BottomCenter).value("1")
                    .items(["Axe", "Spear", "Torch", "Berries", "Water"]),
                Element::new(K::Menu, "hud_side", "").at(Right)
                    .link("Crafting", "crafting", "").link("Backpack", "backpack", ""),
            ];
            bp.screens = vec![
                load,
                main_menu("main_menu", &bp.title, &[("Play", ""), ("Settings", "settings")], ""),
                list("crafting", "Crafting", &[
                    ("Campfire", "5 wood, 3 stone", "craft"),
                    ("Spear", "2 wood, 1 flint", "craft"),
                    ("Water Skin", "2 leather", "craft"),
                ]),
                grid("backpack", "Backpack", &["Wood", "Stone", "Flint", "Berries", "Leather", "Rope", "Empty", "Empty"]),
                results("You Survived", &[("Days", "7"), ("Creatures", "14"), ("Crafted", "22")]),
                settings(),
            ];
        }
        // "simulator" and anything unknown.
        _ => {
            bp.hud = vec![
                Element::new(K::Currency, "hud_coins", "Coins").at(TopRight).value("12.4K").icon("$"),
                Element::new(K::Currency, "hud_gems", "Gems").at(TopRight).value("85").icon("*"),
                Element::new(K::Meter, "hud_backpack", "Backpack").at(BottomCenter).value("38").max("50").icon("o"),
                Element::new(K::Counter, "hud_rebirths", "REBIRTHS").at(TopCenter).value("4"),
                Element::new(K::Menu, "hud_side", "").at(Left)
                    .link("Pets", "pets", "").link("Eggs", "eggs", "").link("Boosts", "boosts", "1").link("Codes", "codes", ""),
            ];
            bp.screens = vec![
                load,
                grid("pets", "Pets", &["Dog", "Cat", "Bunny", "Dragon", "Unicorn", "Empty", "Empty", "Empty"]),
                shop("eggs", "Eggs", &["Eggs", "Gamepasses"], &[
                    ("Basic Egg", "Common pets", "250"),
                    ("Jungle Egg", "Rare pets", "2,500"),
                    ("Mythic Egg", "Legendary odds", "25K"),
                    ("Auto Hatch", "Hatch while you walk", "R$ 199"),
                ]),
                list("boosts", "Boosts", &[
                    ("2x Coins", "30 minutes", "claim"),
                    ("Lucky Hatch", "15 minutes", "claim"),
                    ("Super Speed", "10 minutes", "claim"),
                ]),
                codes(),
                settings(),
            ];
        }
    }
}

// ============================================================================
// Features named in the prompt
// ============================================================================

fn has_element(bp: &Blueprint, id: &str) -> bool {
    bp.hud.iter().any(|e| e.id == id)
}

fn has_screen(bp: &Blueprint, id: &str) -> bool {
    bp.screens.iter().any(|s| s.id == id)
}

fn add_element(bp: &mut Blueprint, e: Element) {
    if !has_element(bp, &e.id) {
        bp.hud.push(e);
    }
}

fn add_screen(bp: &mut Blueprint, s: Screen) {
    if !has_screen(bp, &s.id) {
        bp.screens.push(s);
    }
}

/// Link a screen from the HUD's side menu (or a new one), so everything
/// the prompt asks for can be reached.
fn link_screen(bp: &mut Blueprint, label: &str, screen: &str) {
    if bp.links().iter().any(|(_, s)| s == screen) {
        return;
    }
    if let Some(menu) = bp.hud.iter_mut().find(|e| e.kind == ElementKind::Menu) {
        menu.links.push(Link { label: label.to_string(), opens: screen.to_string(), badge: String::new() });
    } else {
        bp.hud.push(Element::new(ElementKind::Menu, "hud_side", "").at(Anchor::Left).link(label, screen, ""));
    }
}

fn apply_features(text: &str, bp: &mut Blueprint) {
    use Anchor::*;
    use ElementKind as K;
    let any = |words: &[&str]| words.iter().any(|w| contains_word(text, w));

    if any(&["stamina", "sprint"]) {
        add_element(bp, Element::new(K::Meter, "hud_stamina", "Stamina").at(BottomLeft).value("75").max("100").icon(">"));
    }
    if any(&["battery", "flashlight"]) {
        add_element(bp, Element::new(K::Meter, "hud_battery", "Battery").at(BottomLeft).value("80").max("100").icon("!"));
    }
    if any(&["health", "hp", "hearts"]) {
        add_element(bp, Element::new(K::Meter, "hud_health", "Health").at(BottomLeft).value("100").max("100").icon("+"));
    }
    if any(&["mana"]) {
        add_element(bp, Element::new(K::Meter, "hud_mana", "Mana").at(BottomLeft).value("40").max("60").icon("*"));
    }
    if any(&["hunger", "food"]) {
        add_element(bp, Element::new(K::Meter, "hud_hunger", "Hunger").at(BottomLeft).value("60").max("100").icon("o"));
    }
    if any(&["thirst", "water"]) {
        add_element(bp, Element::new(K::Meter, "hud_thirst", "Thirst").at(BottomLeft).value("45").max("100").icon("~"));
    }
    if any(&["fear", "sanity", "vignette"]) {
        add_element(bp, Element::new(K::Meter, "hud_fear", "Fear").at(Right).value("35").max("100").icon("!"));
    }
    if any(&["xp", "experience"]) {
        add_element(bp, Element::new(K::Meter, "hud_xp", "XP").at(BottomCenter).value("620").max("1000").icon("*"));
    }
    if any(&["minimap", "radar"]) {
        add_element(bp, Element::new(K::Minimap, "hud_minimap", "Map").at(TopRight));
    }
    if any(&["timer", "countdown", "round"]) && !bp.hud.iter().any(|e| e.kind == K::Timer) {
        add_element(bp, Element::new(K::Timer, "hud_timer", "TIME").at(TopCenter).value("3:00"));
    }
    if any(&["ammo", "bullets", "magazine"]) {
        add_element(bp, Element::new(K::Counter, "hud_ammo", "AMMO").at(BottomRight).value("30").max("120"));
    }
    if any(&["objective", "goal"]) && !bp.hud.iter().any(|e| e.kind == K::Objective) {
        add_element(bp, Element::new(K::Objective, "hud_objective", "OBJECTIVE").at(TopRight).value("Reach the exit"));
    }
    if any(&["wave", "waves"]) {
        add_element(bp, Element::new(K::Counter, "hud_wave", "WAVE").at(TopCenter).value("1"));
    }
    if any(&["lap", "laps"]) {
        add_element(bp, Element::new(K::Counter, "hud_lap", "LAP").at(TopCenter).value("1").max("3"));
    }
    if any(&["speed", "speedometer"]) && bp.genre == "racing" {
        add_element(bp, Element::new(K::Counter, "hud_speed", "KM/H").at(BottomRight).value("0"));
    }
    if any(&["hotbar", "weapons", "tools"]) && !bp.hud.iter().any(|e| e.kind == K::Hotbar) {
        add_element(bp, Element::new(K::Hotbar, "hud_hotbar", "Items").at(BottomCenter).value("1").items(["1", "2", "3", "4", "5"]));
    }
    // Currencies the prompt names, up to two.
    for (word, label, icon) in [("gems", "Gems", "*"), ("gold", "Gold", "$"), ("cash", "Cash", "$"), ("money", "Money", "$"), ("coins", "Coins", "$"), ("tokens", "Tokens", "*")] {
        let currencies = bp.hud.iter().filter(|e| e.kind == K::Currency).count();
        let already = bp.hud.iter().any(|e| e.kind == K::Currency && e.label.eq_ignore_ascii_case(label));
        if contains_word(text, word) && !already && currencies < 2 {
            add_element(bp, Element::new(K::Currency, &format!("hud_{word}"), label).at(TopRight).value("0").icon(icon));
        }
    }

    // Screens.
    if any(&["emote", "emotes", "dance"]) {
        add_screen(bp, grid("emotes", "Emotes", &["Wave", "Dance", "Salute", "Laugh", "Floss", "Sit"]));
        link_screen(bp, "Emotes", "emotes");
    }
    if any(&["shop", "store"]) && !bp.screens.iter().any(|s| s.kind == ScreenKind::Shop) {
        add_screen(bp, shop("shop", "Shop", &["Featured", "Items"], &[
            ("Starter Pack", "Everything to get going", "R$ 99"),
            ("Speed Boost", "Run 25% faster", "250"),
            ("Double Coins", "Earn twice as much", "R$ 199"),
            ("Mystery Box", "One random item", "500"),
        ]));
        link_screen(bp, "Shop", "shop");
    }
    if any(&["codes", "code", "redeem"]) {
        add_screen(bp, codes());
        link_screen(bp, "Codes", "codes");
    }
    if any(&["pass", "battlepass", "season"]) {
        add_screen(bp, pass(&["Sticker", "100 Coins", "Emote", "Trail", "Pet", "Title"]));
        link_screen(bp, "Pass", "pass");
    }
    if any(&["leaderboard", "scoreboard", "ranking", "ranked"]) && !bp.screens.iter().any(|s| s.kind == ScreenKind::Leaderboard) {
        add_screen(bp, Screen::new("leaderboard", "Leaderboard", ScreenKind::Leaderboard).items(vec![
            Item::new("Player1", "", "12,400"), Item::new("Player2", "", "9,800"),
            Item::new("Player3", "", "7,150"), Item::new("You", "", "2,300"),
        ]));
        link_screen(bp, "Leaderboard", "leaderboard");
    }
    if any(&["inventory", "backpack", "bag"]) && !bp.screens.iter().any(|s| s.id == "inventory" || s.id == "backpack") {
        add_screen(bp, grid("inventory", "Inventory", &["Slot 1", "Slot 2", "Slot 3", "Slot 4", "Slot 5", "Slot 6"]));
        link_screen(bp, "Inventory", "inventory");
    }
    if any(&["daily", "rewards", "login"]) && !has_screen(bp, "daily") {
        add_screen(bp, grid("daily", "Daily Rewards", &["Day 1", "Day 2", "Day 3", "Day 4", "Day 5", "Day 6", "Day 7"]));
        link_screen(bp, "Daily", "daily");
    }
    if any(&["quest", "quests", "mission", "missions", "challenge", "challenges"])
        && !bp.screens.iter().any(|s| s.kind == ScreenKind::List)
    {
        add_screen(bp, list("quests", "Quests", &[
            ("First steps", "Reward: 100 coins", "1/1"),
            ("Explorer", "Visit 5 places", "2/5"),
            ("Collector", "Find 20 items", "7/20"),
        ]));
        link_screen(bp, "Quests", "quests");
    }
    if any(&["main menu", "title screen", "menu"]) && !bp.screens.iter().any(|s| s.kind == ScreenKind::MainMenu) {
        let mut buttons: Vec<(String, String)> = vec![("Play".to_string(), String::new())];
        for s in bp.screens.iter().filter(|s| !s.kind.is_fullscreen() && s.kind != ScreenKind::Results).take(4) {
            buttons.push((s.title.clone(), s.id.clone()));
        }
        let refs: Vec<(&str, &str)> = buttons.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let title = bp.title.clone();
        add_screen(bp, main_menu("main_menu", &title, &refs, ""));
    }
    if any(&["rebirth", "prestige"]) && !has_screen(bp, "rebirth") {
        add_screen(bp, Screen::new("rebirth", "Rebirth", ScreenKind::Results).items(vec![
            Item::new("Next multiplier", "", "x2"), Item::new("Cost", "", "1M"),
        ]));
        link_screen(bp, "Rebirth", "rebirth");
    }
}

/// The last pass: unique element ids, every screen that is not the
/// loading screen reachable from somewhere, settings always present.
pub fn tidy(bp: &mut Blueprint) {
    if !has_screen(bp, "settings") {
        bp.screens.push(settings());
    }
    let mut seen = std::collections::HashSet::new();
    for e in &mut bp.hud {
        let mut id = if e.id.is_empty() { format!("hud_{}", e.kind.id()) } else { e.id.clone() };
        let base = id.clone();
        let mut n = 2;
        while !seen.insert(id.clone()) {
            id = format!("{base}_{n}");
            n += 1;
        }
        e.id = id;
    }
    let orphans: Vec<(String, String)> = bp
        .screens
        .iter()
        .filter(|s| !s.kind.is_fullscreen() && s.kind != ScreenKind::Results)
        .filter(|s| !bp.links().iter().any(|(_, to)| to == &s.id))
        .map(|s| (s.title.clone(), s.id.clone()))
        .collect();
    for (title, id) in orphans {
        link_screen(bp, &title, &id);
    }
}

// ============================================================================
// Design brief
// ============================================================================

/// One numbered section of the design brief.
#[derive(Debug, Clone)]
pub struct BriefSection {
    pub title: String,
    pub body: String,
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// The twelve-section design brief the Create tab shows before and after
/// generating: what the builder decided and why, in words.
pub fn brief(bp: &Blueprint) -> Vec<BriefSection> {
    let genre = catalog::genre(&bp.genre);
    let style = catalog::style(&bp.style).unwrap_or(&catalog::STYLES[0]);
    let kit = resolved_kit(bp);
    let p = &bp.palette;
    let mut out = Vec::new();
    let mut push = |title: String, body: String| out.push(BriefSection { title, body });

    push(
        "Game identity".to_string(),
        format!(
            "{}: a {} game. {}{}",
            bp.title,
            genre.map(|g| g.label).unwrap_or("custom").to_lowercase(),
            bp.tagline,
            if bp.prompt.is_empty() { String::new() } else { format!("\nFrom the prompt: \"{}\"", bp.prompt) },
        ),
    );
    push(format!("Design direction: {}", style.label.to_uppercase()), format!("{}.", style.about));
    push(
        format!("Colour palette: {}", p.label.to_uppercase()),
        format!(
            "Primary {} for the main buttons, secondary {} for the other team and second buttons, accent {} for currency and progress, dark {} for panels, light {} for text.",
            hex(p.primary), hex(p.secondary), hex(p.accent), hex(p.dark), hex(p.light)
        ),
    );
    push(
        "Typography".to_string(),
        format!("Headings in {}, everything else in {}. Numbers are the biggest text on the HUD; captions are small and {}.", bp.heading_font, bp.body_font, if kit.upper { "in capitals" } else { "in sentence case" }),
    );
    push("Logo".to_string(), format!("{} set in {}, stacked on two lines on the loading and main menu screens, light on the dark backdrop.", bp.title.to_uppercase(), bp.heading_font));
    push("Buttons".to_string(), style.buttons.to_string());
    push(
        "Panels, corners and decoration".to_string(),
        format!("{} HUD kit: {} {}", kit.label, kit.about, style.panels),
    );
    push("Click FX and feel".to_string(), style.feel.to_string());
    push("Transitions".to_string(), style.transitions.to_string());

    let visible: Vec<&Element> = bp.hud.iter().filter(|e| e.visible).collect();
    let density = if visible.len() <= 6 { "MINIMAL" } else if visible.len() <= 9 { "BALANCED" } else { "BUSY" };
    let mut corners: Vec<String> = Vec::new();
    for a in Anchor::ALL {
        let here: Vec<String> = visible.iter().filter(|e| e.anchor == a).map(|e| e.display_title()).collect();
        if !here.is_empty() {
            corners.push(format!("{}: {}", a.id(), here.join(", ")));
        }
    }
    push(format!("HUD layout: {density}"), format!("{} elements, the middle of the screen left clear.\n{}", visible.len(), corners.join("\n")));

    push(
        format!("Frames ({})", bp.screens.len() + 1),
        std::iter::once("HUD".to_string()).chain(bp.screens.iter().map(|s| s.title.clone())).collect::<Vec<_>>().join(", "),
    );
    let links = bp.links();
    let flow = if links.is_empty() {
        "No buttons open screens yet.".to_string()
    } else {
        links
            .iter()
            .map(|(label, to)| format!("{} opens {}", label, bp.screen(to).map(|s| s.title.as_str()).unwrap_or(to)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    push("Navigation flow".to_string(), format!("{flow}\nEvery screen but the loading and main menu has a Close button back to the game."));
    out
}

/// The kit the HUD draws with: the blueprint's own, or its style's when
/// it is "auto".
pub fn resolved_kit(bp: &Blueprint) -> &'static catalog::KitDef {
    let id = if bp.kit.is_empty() || bp.kit == "auto" {
        catalog::style(&bp.style).map(|s| s.kit).unwrap_or("original")
    } else {
        bp.kit.as_str()
    };
    catalog::kit(id).unwrap_or(&catalog::KITS[1])
}

// ============================================================================
// Export checklist
// ============================================================================

/// One line of the Export checklist: what the game still has to wire.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckItem {
    pub label: String,
    pub count: usize,
    /// "error", "warn" or "info".
    pub level: &'static str,
    pub detail: String,
}

/// Whether a price is a Robux price (a product or pass the game must
/// create), rather than an in-game currency price.
pub fn is_robux_price(value: &str) -> bool {
    let v = value.trim();
    v.starts_with("R$") || v.contains("Robux")
}

pub fn checklist(bp: &Blueprint) -> Vec<CheckItem> {
    let mut robux = Vec::new();
    let mut grants = Vec::new();
    let mut handled = Vec::new();
    for s in &bp.screens {
        match s.kind {
            ScreenKind::Shop => {
                for i in &s.items {
                    if is_robux_price(&i.value) {
                        robux.push(format!("{} ({})", i.name, s.title));
                    } else {
                        grants.push(format!("{} ({})", i.name, s.title));
                    }
                }
            }
            ScreenKind::Pass => robux.push(format!("{} premium track", s.title)),
            ScreenKind::List => {
                for i in &s.items {
                    grants.push(format!("{} reward ({})", i.name, s.title));
                }
            }
            ScreenKind::Codes => handled.push("Redeem (Codes)".to_string()),
            ScreenKind::MainMenu => {
                for i in s.items.iter().filter(|i| i.opens.is_empty()) {
                    handled.push(format!("{} ({})", i.name, s.title));
                }
            }
            _ => {}
        }
    }
    for e in &bp.hud {
        for l in e.links.iter().filter(|l| l.opens.is_empty()) {
            handled.push(format!("{} (HUD)", l.label));
        }
    }
    let values: Vec<String> = bp
        .hud
        .iter()
        .filter(|e| !matches!(e.kind, ElementKind::Button | ElementKind::Menu | ElementKind::Controls | ElementKind::Minimap))
        .map(|e| e.id.clone())
        .collect();
    let mut out = Vec::new();
    if !robux.is_empty() {
        out.push(CheckItem {
            label: "Products and game passes to create".to_string(),
            count: robux.len(),
            level: "error",
            detail: format!("Robux prices need a product or pass id (MarketplaceService): {}", robux.join(", ")),
        });
    }
    if !grants.is_empty() {
        out.push(CheckItem {
            label: "Items without a grant".to_string(),
            count: grants.len(),
            level: "warn",
            detail: format!("Handle them with UI.on(\"buy\") or UI.on(\"claim\"): {}", grants.join(", ")),
        });
    }
    if !handled.is_empty() {
        out.push(CheckItem {
            label: "Buttons your game handles".to_string(),
            count: handled.len(),
            level: "warn",
            detail: format!("Give each a UI.on handler: {}", handled.join(", ")),
        });
    }
    if !values.is_empty() {
        out.push(CheckItem {
            label: "Values your game sets".to_string(),
            count: values.len(),
            level: "info",
            detail: format!("Call UI.set(id, value) as they change: {}", values.join(", ")),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_prompt_gives_the_same_ui() {
        let p = "A tense horror escape game like Doors: minimal typewriter HUD, flashlight battery, stamina";
        let a = generate(p, seed_for(p));
        let b = generate(p, seed_for(p));
        assert_eq!(a, b);
        assert_eq!(a.genre, "horror");
        assert_eq!(a.style, "horror");
        assert!(a.hud.iter().any(|e| e.id == "hud_stamina"));
        assert!(a.hud.iter().any(|e| e.id == "hud_battery"));
    }

    #[test]
    fn a_variation_changes_the_look_not_the_game() {
        let p = "a cartoony battle royale with emotes and a season pass";
        let a = generate(p, seed_for(p));
        let b = generate(p, seed_for(p) + 1);
        assert_eq!(a.genre, b.genre);
        assert_eq!(a.hud.len(), b.hud.len());
        assert!(a.screens.iter().any(|s| s.id == "emotes"));
    }

    #[test]
    fn every_screen_can_be_reached() {
        for g in catalog::GENRES {
            let bp = generate(g.example, seed_for(g.example));
            let links = bp.links();
            for s in bp.screens.iter().filter(|s| !s.kind.is_fullscreen() && s.kind != ScreenKind::Results) {
                assert!(links.iter().any(|(_, to)| to == &s.id), "{}: {} unreachable", g.id, s.id);
            }
            for (_, to) in &links {
                assert!(bp.screen(to).is_some(), "{}: link to missing {}", g.id, to);
            }
            let ids: std::collections::HashSet<_> = bp.hud.iter().map(|e| &e.id).collect();
            assert_eq!(ids.len(), bp.hud.len(), "{}: duplicate element ids", g.id);
        }
    }

    #[test]
    fn a_named_title_is_kept() {
        let bp = generate("a racing game called Turbo Town, with laps", 1234);
        assert_eq!(bp.title, "Turbo Town");
        assert_eq!(bp.name, "TurboTown");
        let bp = generate("an obby named \"Sky Climb\"", 1234);
        assert_eq!(bp.title, "Sky Climb");
    }

    #[test]
    fn the_brief_has_twelve_sections() {
        let bp = generate("a pet simulator", 1111);
        assert_eq!(brief(&bp).len(), 12);
        assert!(!checklist(&bp).is_empty());
    }
}
