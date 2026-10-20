//! What a generated UI is, as data: the [`Blueprint`].
//!
//! A blueprint is everything the UI Builder knows about one game's UI: its
//! look (style, HUD kit, palette, fonts), the HUD elements with where each
//! sits on screen, and the screens (menus, shop, settings, ...) the HUD's
//! buttons open. The panel edits it, the preview draws it
//! ([`super::layout`]), and Insert writes it into the Space as ScreenGuis
//! ([`super::emit`]). It is saved beside the Space as JSON, so a UI can be
//! loaded again and changed after it was inserted.

use serde::{Deserialize, Serialize};

/// Bumped when a field changes meaning; older files still load, because
/// every field has a default.
pub const BLUEPRINT_VERSION: u32 = 1;

/// An sRGB colour, 0 to 255 per channel.
pub type Rgb = [u8; 3];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Blueprint {
    pub version: u32,
    /// Letters and digits only: names the ScreenGuis, the scripts and the
    /// saved file, so scripts can find the UI by it.
    pub name: String,
    /// The game's title as players read it: "The Silent Inn".
    pub title: String,
    /// One line under the title on the loading and main menu screens.
    pub tagline: String,
    /// What the user typed.
    pub prompt: String,
    /// A genre id from [`super::catalog::GENRES`].
    pub genre: String,
    /// A style id from [`super::catalog::STYLES`].
    pub style: String,
    /// The HUD kit every element uses unless it names its own.
    pub kit: String,
    pub palette: Palette,
    pub heading_font: String,
    pub body_font: String,
    /// Picks the title and the order of optional elements; Variation
    /// changes it.
    pub seed: u64,
    /// `"offline"` or `"claude"`: who wrote this blueprint.
    pub source: String,
    pub hud: Vec<Element>,
    pub screens: Vec<Screen>,
}

impl Default for Blueprint {
    fn default() -> Self {
        Self {
            version: BLUEPRINT_VERSION,
            name: "GameUI".to_string(),
            title: "My Game".to_string(),
            tagline: String::new(),
            prompt: String::new(),
            genre: "simulator".to_string(),
            style: "arena".to_string(),
            kit: "auto".to_string(),
            palette: Palette::default(),
            heading_font: "GothamBlack".to_string(),
            body_font: "GothamBold".to_string(),
            seed: 1,
            source: "offline".to_string(),
            hud: Vec::new(),
            screens: Vec::new(),
        }
    }
}

impl Blueprint {
    pub fn screen(&self, id: &str) -> Option<&Screen> {
        self.screens.iter().find(|s| s.id == id)
    }

    /// Every button that opens a screen, as (button label, screen id), HUD
    /// first. What the navigation script wires and the brief describes.
    pub fn links(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for e in &self.hud {
            if !e.opens.is_empty() {
                out.push((e.label.clone(), e.opens.clone()));
            }
            for l in &e.links {
                if !l.opens.is_empty() {
                    out.push((l.label.clone(), l.opens.clone()));
                }
            }
        }
        for s in &self.screens {
            for item in &s.items {
                if !item.opens.is_empty() {
                    out.push((item.name.clone(), item.opens.clone()));
                }
            }
        }
        out
    }
}

/// Five colours with fixed jobs, so every kit can paint any palette.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Palette {
    pub id: String,
    pub label: String,
    /// Brand colour: primary buttons, the main plate tint in bold kits.
    pub primary: Rgb,
    /// Second brand colour: the other team, secondary buttons.
    pub secondary: Rgb,
    /// Highlights: currency, progress fills, the selected slot.
    pub accent: Rgb,
    /// Panels and backdrops.
    pub dark: Rgb,
    /// Text on dark panels.
    pub light: Rgb,
}

impl Default for Palette {
    fn default() -> Self {
        super::catalog::palette("arena_night")
            .map(|p| p.to_palette())
            .unwrap_or(Self {
                id: "arena_night".to_string(),
                label: "Arena Night".to_string(),
                primary: [255, 170, 40],
                secondary: [230, 60, 70],
                accent: [60, 150, 255],
                dark: [18, 22, 34],
                light: [255, 255, 255],
            })
    }
}

/// What a HUD element is. Each kind has its own drawing in
/// [`super::layout`]; the fields it reads are listed on each variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    /// A wallet line: `icon`, `value`, `label` (the currency name).
    #[default]
    Currency,
    /// A big number with a caption: `label`, `value`, `max` (goal),
    /// `digits` (zero padding).
    Counter,
    /// The current goal: `label` (caption), `value` (the goal's text).
    Objective,
    /// A bar: `label`, `icon`, `value` and `max` as numbers.
    Meter,
    /// A clock: `label`, `value` ("03:13").
    Timer,
    /// Numbered slots: `items` (slot names), `value` (selected slot, 1-based).
    Hotbar,
    /// Two team scores and a VS: `items` = [left name, right name],
    /// `value` = "39-45".
    TeamScore,
    /// A square map with the player at the centre: `label`.
    Minimap,
    /// One button that opens a screen: `label`, `icon`, `opens`.
    Button,
    /// A stack of buttons, each opening a screen: `links`.
    Menu,
    /// A checklist: `label`, `items` ("Collect 20 batteries|20/20").
    Tracker,
    /// Key hints: `items` ("Tab|Scoreboard").
    Controls,
    /// A banner across the top: `label`, `value`. Also what a kind this
    /// version does not know reads as, so a newer or hand-written file
    /// still loads.
    #[serde(other)]
    Banner,
}

impl ElementKind {
    pub const ALL: [ElementKind; 13] = [
        ElementKind::Currency,
        ElementKind::Counter,
        ElementKind::Objective,
        ElementKind::Meter,
        ElementKind::Timer,
        ElementKind::Hotbar,
        ElementKind::TeamScore,
        ElementKind::Minimap,
        ElementKind::Button,
        ElementKind::Menu,
        ElementKind::Tracker,
        ElementKind::Controls,
        ElementKind::Banner,
    ];

    pub fn id(self) -> &'static str {
        match self {
            ElementKind::Currency => "currency",
            ElementKind::Counter => "counter",
            ElementKind::Objective => "objective",
            ElementKind::Meter => "meter",
            ElementKind::Timer => "timer",
            ElementKind::Hotbar => "hotbar",
            ElementKind::TeamScore => "team_score",
            ElementKind::Minimap => "minimap",
            ElementKind::Button => "button",
            ElementKind::Menu => "menu",
            ElementKind::Tracker => "tracker",
            ElementKind::Controls => "controls",
            ElementKind::Banner => "banner",
        }
    }

    pub fn from_id(id: &str) -> Option<ElementKind> {
        ElementKind::ALL.iter().copied().find(|k| k.id() == id)
    }

    /// How the Structure tab names the kind.
    pub fn title(self) -> &'static str {
        match self {
            ElementKind::Currency => "Currency",
            ElementKind::Counter => "Counter",
            ElementKind::Objective => "Objective",
            ElementKind::Meter => "Meter",
            ElementKind::Timer => "Timer",
            ElementKind::Hotbar => "Hotbar",
            ElementKind::TeamScore => "Team score",
            ElementKind::Minimap => "Minimap",
            ElementKind::Button => "Button",
            ElementKind::Menu => "Menu",
            ElementKind::Tracker => "Tracker",
            ElementKind::Controls => "Controls",
            ElementKind::Banner => "Banner",
        }
    }

    /// The icon file (under `assets/icons/ui/`) the Structure tab shows.
    pub fn icon(self) -> &'static str {
        match self {
            ElementKind::Currency => "coin",
            ElementKind::Counter => "sigma",
            ElementKind::Objective => "flag",
            ElementKind::Meter => "gauge",
            ElementKind::Timer => "clock",
            ElementKind::Hotbar => "array-linear",
            ElementKind::TeamScore => "trophy",
            ElementKind::Minimap => "map",
            ElementKind::Button => "button",
            ElementKind::Menu => "list",
            ElementKind::Tracker => "checklist",
            ElementKind::Controls => "keyboard",
            ElementKind::Banner => "megaphone",
        }
    }

    /// Where a new element of this kind goes when the user adds one.
    pub fn default_anchor(self) -> Anchor {
        match self {
            ElementKind::Currency => Anchor::TopRight,
            ElementKind::Counter => Anchor::TopCenter,
            ElementKind::Objective => Anchor::TopLeft,
            ElementKind::Meter => Anchor::BottomLeft,
            ElementKind::Timer => Anchor::TopCenter,
            ElementKind::Hotbar => Anchor::BottomCenter,
            ElementKind::TeamScore => Anchor::TopCenter,
            ElementKind::Minimap => Anchor::TopRight,
            ElementKind::Button => Anchor::TopLeft,
            ElementKind::Menu => Anchor::Left,
            ElementKind::Tracker => Anchor::TopLeft,
            ElementKind::Controls => Anchor::BottomRight,
            ElementKind::Banner => Anchor::TopCenter,
        }
    }
}

/// Nine places on the screen. Elements that share one stack away from the
/// edge in list order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Anchor {
    TopCenter,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    BottomCenter,
    BottomRight,
    /// Last because `#[serde(other)]` must be: also what an unknown anchor
    /// name reads as. `Anchor::ALL` keeps the reading order.
    #[default]
    #[serde(other)]
    TopLeft,
}

impl Anchor {
    pub const ALL: [Anchor; 9] = [
        Anchor::TopLeft,
        Anchor::TopCenter,
        Anchor::TopRight,
        Anchor::Left,
        Anchor::Center,
        Anchor::Right,
        Anchor::BottomLeft,
        Anchor::BottomCenter,
        Anchor::BottomRight,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Anchor::TopLeft => "TopLeft",
            Anchor::TopCenter => "TopCenter",
            Anchor::TopRight => "TopRight",
            Anchor::Left => "Left",
            Anchor::Center => "Center",
            Anchor::Right => "Right",
            Anchor::BottomLeft => "BottomLeft",
            Anchor::BottomCenter => "BottomCenter",
            Anchor::BottomRight => "BottomRight",
        }
    }

    pub fn from_id(id: &str) -> Option<Anchor> {
        Anchor::ALL.iter().copied().find(|a| a.id().eq_ignore_ascii_case(id))
    }

    /// The screen point the element hangs from, as a fraction of the
    /// screen: also its `AnchorPoint`.
    pub fn fraction(self) -> (f32, f32) {
        match self {
            Anchor::TopLeft => (0.0, 0.0),
            Anchor::TopCenter => (0.5, 0.0),
            Anchor::TopRight => (1.0, 0.0),
            Anchor::Left => (0.0, 0.5),
            Anchor::Center => (0.5, 0.5),
            Anchor::Right => (1.0, 0.5),
            Anchor::BottomLeft => (0.0, 1.0),
            Anchor::BottomCenter => (0.5, 1.0),
            Anchor::BottomRight => (1.0, 1.0),
        }
    }
}

/// A labelled button that opens a screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Link {
    pub label: String,
    /// A screen id; empty for a button the game handles itself.
    pub opens: String,
    /// A small red count on the button ("2"), empty for none.
    pub badge: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Element {
    /// Scripts address the element by it: `UI.set("hud_counter", 42)`.
    pub id: String,
    pub kind: ElementKind,
    pub anchor: Anchor,
    pub label: String,
    pub value: String,
    pub max: String,
    /// One or two characters drawn in the element's badge ("$", "+").
    pub icon: String,
    /// Zero padding for counters: 4 shows 0042.
    pub digits: u32,
    pub items: Vec<String>,
    pub links: Vec<Link>,
    /// The screen a Button opens.
    pub opens: String,
    /// A kit id that overrides the blueprint's kit for this element; empty
    /// for the kit's choice.
    pub look: String,
    pub visible: bool,
}

impl Default for Element {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: ElementKind::Currency,
            anchor: Anchor::TopLeft,
            label: String::new(),
            value: String::new(),
            max: String::new(),
            icon: String::new(),
            digits: 0,
            items: Vec::new(),
            links: Vec::new(),
            opens: String::new(),
            look: String::new(),
            visible: true,
        }
    }
}

impl Element {
    pub fn new(kind: ElementKind, id: &str, label: &str) -> Self {
        Self {
            id: id.to_string(),
            kind,
            anchor: kind.default_anchor(),
            label: label.to_string(),
            ..Default::default()
        }
    }

    pub fn at(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    pub fn value(mut self, v: impl Into<String>) -> Self {
        self.value = v.into();
        self
    }

    pub fn max(mut self, v: impl Into<String>) -> Self {
        self.max = v.into();
        self
    }

    pub fn icon(mut self, v: impl Into<String>) -> Self {
        self.icon = v.into();
        self
    }

    pub fn digits(mut self, d: u32) -> Self {
        self.digits = d;
        self
    }

    pub fn items<S: Into<String>>(mut self, items: impl IntoIterator<Item = S>) -> Self {
        self.items = items.into_iter().map(Into::into).collect();
        self
    }

    pub fn opens(mut self, screen: &str) -> Self {
        self.opens = screen.to_string();
        self
    }

    pub fn link(mut self, label: &str, opens: &str, badge: &str) -> Self {
        self.links.push(Link { label: label.to_string(), opens: opens.to_string(), badge: badge.to_string() });
        self
    }

    /// "Counter: ROOM", the Structure tab's row title.
    pub fn display_title(&self) -> String {
        if self.label.is_empty() {
            self.kind.title().to_string()
        } else {
            format!("{}: {}", self.kind.title(), self.label)
        }
    }
}

/// How a screen is laid out. Each has its own drawing in
/// [`super::layout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScreenKind {
    /// Full screen: title, progress bar, a tip.
    Loading,
    /// Full screen: title, a column of buttons (`items` with `opens`), a
    /// featured offer (`tabs[0]`, if any).
    MainMenu,
    /// Category tabs (`tabs`) and item cards with a price (`items`).
    Shop,
    /// A grid of slots (`items`).
    #[default]
    Grid,
    /// Rows with a progress bar and a Claim button (`items`, value "3/8").
    List,
    /// Category tabs (`tabs`) and switch rows (`items`, value "on"/"off").
    Settings,
    /// A result title and stat rows (`items`, value), Play again / Lobby.
    Results,
    /// A text box and a Redeem button.
    Codes,
    /// A row of reward tiers (`items`).
    Pass,
    /// Ranked rows (`items`, value).
    Leaderboard,
    /// Paragraphs of text (`items`). Also what a kind this version does
    /// not know reads as.
    #[serde(other)]
    Info,
}

impl ScreenKind {
    pub const ALL: [ScreenKind; 11] = [
        ScreenKind::Loading,
        ScreenKind::MainMenu,
        ScreenKind::Shop,
        ScreenKind::Grid,
        ScreenKind::List,
        ScreenKind::Settings,
        ScreenKind::Results,
        ScreenKind::Codes,
        ScreenKind::Pass,
        ScreenKind::Leaderboard,
        ScreenKind::Info,
    ];

    pub fn id(self) -> &'static str {
        match self {
            ScreenKind::Loading => "loading",
            ScreenKind::MainMenu => "main_menu",
            ScreenKind::Shop => "shop",
            ScreenKind::Grid => "grid",
            ScreenKind::List => "list",
            ScreenKind::Settings => "settings",
            ScreenKind::Results => "results",
            ScreenKind::Codes => "codes",
            ScreenKind::Pass => "pass",
            ScreenKind::Leaderboard => "leaderboard",
            ScreenKind::Info => "info",
        }
    }

    pub fn from_id(id: &str) -> Option<ScreenKind> {
        ScreenKind::ALL.iter().copied().find(|k| k.id() == id)
    }

    pub fn icon(self) -> &'static str {
        match self {
            ScreenKind::Loading => "refresh",
            ScreenKind::MainMenu => "layout-default",
            ScreenKind::Shop => "coin",
            ScreenKind::Grid => "array-grid",
            ScreenKind::List => "checklist",
            ScreenKind::Settings => "settings",
            ScreenKind::Results => "trophy",
            ScreenKind::Codes => "key",
            ScreenKind::Pass => "certificate",
            ScreenKind::Leaderboard => "chart-bar",
            ScreenKind::Info => "book",
        }
    }

    /// Whether the screen covers the whole view (and so has no Close).
    pub fn is_fullscreen(self) -> bool {
        matches!(self, ScreenKind::Loading | ScreenKind::MainMenu)
    }
}

/// One entry on a screen: a shop item, a quest, a setting, a stat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Item {
    pub name: String,
    pub detail: String,
    /// A price, a progress ("3/8"), a setting ("on"), a stat ("12").
    pub value: String,
    pub icon: String,
    /// A screen this entry's button opens (main menu buttons).
    pub opens: String,
}

impl Item {
    pub fn new(name: &str, detail: &str, value: &str) -> Self {
        Self { name: name.to_string(), detail: detail.to_string(), value: value.to_string(), ..Default::default() }
    }

    pub fn opens(mut self, screen: &str) -> Self {
        self.opens = screen.to_string();
        self
    }

    pub fn icon(mut self, icon: &str) -> Self {
        self.icon = icon.to_string();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Screen {
    /// Letters, digits and underscores: names the ScreenGui and is what
    /// `UI.open("shop")` takes.
    pub id: String,
    pub title: String,
    pub kind: ScreenKind,
    pub tabs: Vec<String>,
    pub items: Vec<Item>,
}

impl Screen {
    pub fn new(id: &str, title: &str, kind: ScreenKind) -> Self {
        Self { id: id.to_string(), title: title.to_string(), kind, ..Default::default() }
    }

    pub fn tabs<S: Into<String>>(mut self, tabs: impl IntoIterator<Item = S>) -> Self {
        self.tabs = tabs.into_iter().map(Into::into).collect();
        self
    }

    pub fn items(mut self, items: Vec<Item>) -> Self {
        self.items = items;
        self
    }
}

/// Letters and digits only, capitalised at each word: "the silent inn" is
/// "TheSilentInn". Empty input gives "GameUI".
pub fn pascal_name(raw: &str) -> String {
    let mut out = String::new();
    for word in raw.split(|c: char| !c.is_ascii_alphanumeric()) {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
        }
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert_str(0, "UI");
    }
    out.truncate(40);
    out
}

/// Lowercase letters, digits and underscores: "Pre-Run Shop" is
/// "pre_run_shop".
pub fn snake_id(raw: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('_');
            }
            gap = false;
            out.push(c.to_ascii_lowercase());
        } else {
            gap = true;
        }
    }
    if out.is_empty() {
        out.push_str("screen");
    }
    out.truncate(40);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_script_safe() {
        assert_eq!(pascal_name("the silent inn"), "TheSilentInn");
        assert_eq!(pascal_name("  "), "UI");
        assert_eq!(pascal_name("3 Doors"), "UI3Doors");
        assert_eq!(snake_id("Pre-Run Shop"), "pre_run_shop");
        assert_eq!(snake_id("!!"), "screen");
    }

    #[test]
    fn a_blueprint_round_trips_through_json() {
        let mut b = Blueprint::default();
        b.hud.push(Element::new(ElementKind::Counter, "hud_counter", "ROOM").value("79").digits(4));
        b.screens.push(Screen::new("shop", "Shop", ScreenKind::Shop).items(vec![Item::new("Sword", "", "250")]));
        let json = serde_json::to_string(&b).unwrap();
        let back: Blueprint = serde_json::from_str(&json).unwrap();
        assert_eq!(b, back);
    }

    #[test]
    fn a_partial_blueprint_fills_its_defaults() {
        let back: Blueprint = serde_json::from_str(r#"{"name":"X","hud":[{"kind":"meter","id":"hp"}]}"#).unwrap();
        assert_eq!(back.name, "X");
        assert_eq!(back.hud[0].kind, ElementKind::Meter);
        assert!(back.hud[0].visible);
    }
}
