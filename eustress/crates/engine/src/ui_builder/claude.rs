//! Claude AI mode: Claude writes the blueprint from the prompt, and can
//! rewrite a one-line prompt into a fuller one.
//!
//! Calls run on a background thread through the engine's own
//! [`crate::soul::ClaudeClient`] with the key from Settings > Soul, the
//! same way the Workshop's steps do, and land in an [`AiJob`] the builder
//! polls each frame. Whatever Claude returns goes through [`parse_blueprint`],
//! which keeps only what the catalogue and the layout understand, so a
//! wrong id or a dangling link can never reach the Space.

use std::sync::{Arc, Mutex};

use eustress_common::soul::ClaudeConfig;

use super::blueprint::*;
use super::catalog;

/// What an in-flight call is for.
#[derive(Debug, Clone, PartialEq)]
pub enum AiPurpose {
    /// A blueprint; `apply` makes it the result, else it is only the brief.
    Blueprint { apply: bool },
    /// A rewritten prompt.
    ImprovePrompt,
}

pub struct AiJob {
    pub purpose: AiPurpose,
    pub prompt: String,
    pub seed: u64,
    pub started: std::time::Instant,
    result: Arc<Mutex<Option<Result<String, String>>>>,
}

impl AiJob {
    /// The answer, once; `None` while Claude is still writing.
    pub fn take(&self) -> Option<Result<String, String>> {
        match self.result.lock() {
            Ok(mut slot) => slot.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        }
    }
}

fn spawn(api_key: String, purpose: AiPurpose, prompt: String, seed: u64, user: String, system: &'static str, cheap: bool) -> AiJob {
    let result: Arc<Mutex<Option<Result<String, String>>>> = Arc::new(Mutex::new(None));
    let slot = result.clone();
    let config = ClaudeConfig { api_key: Some(api_key), ..ClaudeConfig::default() };
    std::thread::spawn(move || {
        let client = crate::soul::ClaudeClient::new(config);
        let answer = if cheap { client.call_api_haiku(&user, system) } else { client.call_api_for_workshop(&user, system) };
        match slot.lock() {
            Ok(mut s) => *s = Some(answer),
            Err(poisoned) => *poisoned.into_inner() = Some(answer),
        }
    });
    AiJob { purpose, prompt, seed, started: std::time::Instant::now(), result }
}

/// Ask Claude for a blueprint.
pub fn start_blueprint(api_key: String, prompt: &str, seed: u64, apply: bool) -> AiJob {
    let user = format!(
        "Game description: {prompt}\n\nVariation seed: {seed} (use it to vary the title and the details when the same description comes back).\n\nReturn the blueprint JSON only.",
    );
    spawn(api_key, AiPurpose::Blueprint { apply }, prompt.to_string(), seed, user, BLUEPRINT_SYSTEM, false)
}

/// Ask Claude to rewrite a short prompt into a fuller one.
pub fn start_improve(api_key: String, prompt: &str) -> AiJob {
    spawn(api_key, AiPurpose::ImprovePrompt, prompt.to_string(), 0, prompt.to_string(), IMPROVE_SYSTEM, true)
}

const IMPROVE_SYSTEM: &str = "You rewrite a game UI request into one richer paragraph (at most 60 words) for a UI generator. \
Keep the user's genre, setting and every feature they named. Add the HUD values and the screens a game like this needs \
(for example currencies, meters, timers, a shop, quests, settings), and one phrase about the look. \
Reply with the paragraph only: no quotes, no preamble, no lists.";

const BLUEPRINT_SYSTEM: &str = r#"You design complete game UIs for Eustress, a Roblox-style engine. From one game description, return ONE JSON object (no markdown fence, no commentary) with this shape:

{
 "title": "The Silent Inn",            // the game's name as players read it
 "tagline": "Every hundredth door is different.",
 "genre": "horror",                    // one of: tycoon rpg obby racing tower_defense battle_royale horror roleplay fps simulator survival
 "style": "horror",                    // one of: arena bubbly adventure sleek candy tactical horror
 "kit": "auto",                        // auto, or one of: original competitive arcade glass chunky_toy minimal neon tactical storybook retro
 "palette": {"label": "Hotel Brass", "primary": [212,176,92], "secondary": [142,40,52], "accent": [132,200,238], "dark": [22,20,20], "light": [240,232,214]},
 "heading_font": "MerriweatherBold", "body_font": "SpecialElite",
 "hud": [ELEMENT, ...],
 "screens": [SCREEN, ...]
}

ELEMENT: {"id": "hud_counter", "kind": KIND, "anchor": ANCHOR, "label": "ROOM", "value": "79", "max": "", "icon": "$", "digits": 0, "items": [], "links": [], "opens": ""}
- ids are unique snake_case starting with "hud_".
- ANCHOR: TopLeft TopCenter TopRight Left Center Right BottomLeft BottomCenter BottomRight. Elements sharing an anchor stack away from the edge.
- KIND and the fields it reads:
  currency: label (currency name), value ("1,250"), icon (1 char), opens (a shop screen id or "")
  counter: label (caption), value, max (goal, may be ""), digits (zero padding)
  objective: label (caption), value (the goal text)
  meter: label, icon, value and max as numbers ("80","100")
  timer: label, value ("3:59")
  hotbar: items (slot names, up to 6), value (selected slot, "1")
  team_score: label (goal text), value ("39-45"), items ["Blue Team","Red Team"]
  minimap: label
  button: label, opens (a screen id)
  menu: links [{"label":"Shop","opens":"shop","badge":""}] (badge: a small count like "2" or "")
  tracker: label, items ["Collect 20 batteries|20/20", ...]
  controls: label, items ["Tab|Scoreboard", ...]
  banner: label, value
- 5 to 10 elements. Keep the middle of the screen clear.

SCREEN: {"id": "shop", "title": "Shop", "kind": SKIND, "tabs": [], "items": [{"name": "Sword", "detail": "+12 attack", "value": "250", "icon": "", "opens": ""}]}
- ids are unique snake_case. SKIND and what it reads:
  loading: items[0].name is a tip
  main_menu: items are buttons; the first is the main action (opens ""), the others open screens by id; tabs[0] (optional) is a featured offer's name
  shop: tabs (categories), items with value = price ("250" in game currency, "R$ 199" for Robux)
  grid: items (slots, up to 12)
  list: items with value = progress ("3/8") or "claim"
  settings: tabs, items with value "on" or "off"
  results: title (e.g. "Victory"), items with value (stats)
  codes: items[0].detail is a hint
  pass: items are reward tiers
  leaderboard: items name/detail/value
  info: items name (heading) and detail (paragraph)
- 6 to 10 screens: always a loading screen and a settings screen. Up to 6 items per screen.
- Every screen except loading, main_menu and results must be opened by some button: an element's "opens", a menu link, or a main menu item.

Match the game's tone in every label. Use short labels that fit on a HUD plate. Return compact JSON."#;

/// The first `{` to the last `}`: Claude's JSON even with prose around it.
fn json_span(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| &text[start..=end])
}

/// Claude's answer as a blueprint the builder can trust: ids made safe and
/// unique, unknown catalogue ids replaced, dangling links dropped, and
/// every screen reachable.
pub fn parse_blueprint(text: &str, prompt: &str, seed: u64) -> Result<Blueprint, String> {
    let span = json_span(text).ok_or_else(|| "Claude's answer had no JSON in it".to_string())?;
    let mut bp: Blueprint = serde_json::from_str(span).map_err(|e| format!("Claude's JSON did not read as a blueprint: {e}"))?;

    bp.version = BLUEPRINT_VERSION;
    bp.prompt = prompt.trim().to_string();
    bp.seed = seed;
    bp.source = "claude".to_string();
    if bp.title.trim().is_empty() {
        bp.title = "My Game".to_string();
    }
    bp.name = pascal_name(&bp.title);

    let genre = catalog::genre(&bp.genre).unwrap_or_else(|| catalog::detect_genre(prompt));
    bp.genre = genre.id.to_string();
    if catalog::style(&bp.style).is_none() {
        bp.style = genre.style.to_string();
    }
    if bp.kit != "auto" && catalog::kit(&bp.kit).is_none() {
        bp.kit = "auto".to_string();
    }
    // A palette id alone, or colours without an id, both work.
    if let Some(p) = catalog::palette(&bp.palette.id) {
        bp.palette = p.to_palette();
    } else if bp.palette.id.is_empty() {
        bp.palette.id = snake_id(&bp.palette.label);
    }
    if bp.palette.label.is_empty() {
        bp.palette.label = "Custom".to_string();
    }
    let style = catalog::style(&bp.style).unwrap_or(&catalog::STYLES[0]);
    if bp.heading_font.is_empty() {
        bp.heading_font = style.heading_font.to_string();
    }
    if bp.body_font.is_empty() {
        bp.body_font = style.body_font.to_string();
    }

    // Screens: safe unique ids, at most 12, 8 items each.
    bp.screens.truncate(12);
    let mut seen = std::collections::HashSet::new();
    for s in &mut bp.screens {
        let mut id = snake_id(if s.id.is_empty() { &s.title } else { &s.id });
        let base = id.clone();
        let mut n = 2;
        while !seen.insert(id.clone()) {
            id = format!("{base}_{n}");
            n += 1;
        }
        s.id = id;
        if s.title.is_empty() {
            s.title = s.id.replace('_', " ");
        }
        s.items.truncate(8);
        s.tabs.truncate(6);
    }
    if !bp.screens.iter().any(|s| s.kind == ScreenKind::Loading) {
        bp.screens.insert(0, Screen::new("loading", "Loading", ScreenKind::Loading).items(vec![Item::new(&bp.tagline, "", "")]));
    }
    let ids: Vec<String> = bp.screens.iter().map(|s| s.id.clone()).collect();
    let known = |to: &str| ids.iter().any(|i| i == to);

    // Links to screens that do not exist lead nowhere: drop them.
    for s in &mut bp.screens {
        for item in &mut s.items {
            if !item.opens.is_empty() && !known(&snake_id(&item.opens)) {
                item.opens.clear();
            } else if !item.opens.is_empty() {
                item.opens = snake_id(&item.opens);
            }
        }
    }
    bp.hud.truncate(14);
    for e in &mut bp.hud {
        if !e.opens.is_empty() {
            e.opens = if known(&snake_id(&e.opens)) { snake_id(&e.opens) } else { String::new() };
        }
        for l in &mut e.links {
            if !l.opens.is_empty() {
                l.opens = if known(&snake_id(&l.opens)) { snake_id(&l.opens) } else { String::new() };
            }
        }
        e.items.truncate(9);
        e.links.truncate(6);
        if !e.look.is_empty() && catalog::kit(&e.look).is_none() {
            e.look.clear();
        }
        e.digits = e.digits.min(9);
    }

    super::generate::tidy(&mut bp);
    Ok(bp)
}

/// Claude's rewritten prompt, cleaned of quotes and preamble.
pub fn parse_improved(text: &str) -> String {
    let t = text.trim().trim_matches('"').trim();
    t.lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_messy_answer_becomes_a_safe_blueprint() {
        let answer = r#"Here you go:
{"title":"Neon Rush","genre":"racing","style":"nope","kit":"weird","palette":{"id":"neon_pulse"},
 "hud":[{"id":"hud_speed","kind":"counter","anchor":"BottomRight","label":"KM/H","value":"200"},
        {"id":"hud_menu","kind":"button","label":"Garage","opens":"Garage"},
        {"id":"hud_bad","kind":"button","label":"Nowhere","opens":"missing"}],
 "screens":[{"id":"Garage","title":"Garage","kind":"grid","items":[{"name":"Car"}]},
            {"id":"settings","title":"Settings","kind":"settings"}]}
Thanks!"#;
        let bp = parse_blueprint(answer, "neon racing", 5).unwrap();
        assert_eq!(bp.name, "NeonRush");
        assert_eq!(bp.style, "arena");
        assert_eq!(bp.kit, "auto");
        assert_eq!(bp.palette.label, "Neon Pulse");
        assert_eq!(bp.screens[0].kind, ScreenKind::Loading);
        assert_eq!(bp.hud.iter().find(|e| e.id == "hud_menu").unwrap().opens, "garage");
        assert!(bp.hud.iter().find(|e| e.id == "hud_bad").unwrap().opens.is_empty());
        // Settings gets a way in even though Claude gave it none.
        assert!(bp.links().iter().any(|(_, to)| to == "settings"));
    }

    #[test]
    fn no_json_is_an_error() {
        assert!(parse_blueprint("I cannot do that", "x", 1).is_err());
    }
}
