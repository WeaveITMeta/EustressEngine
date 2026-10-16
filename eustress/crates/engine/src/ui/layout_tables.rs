//! Studio's panel layout per mode and discipline.
//!
//! A mode's manifest can carry a layout table (`[layout.zones]`, and a
//! discipline's own in `[[submode_layouts]]`, which replaces the mode's
//! whole). Switching mode or discipline swaps the layout in:
//!
//! 1. the layout the user left this mode and discipline in, if any
//!    (remembered per mode and discipline in `EditorSettings`);
//! 2. otherwise the discipline's table, else the mode's; a mode with no
//!    table keeps the docks as they are;
//! 3. then pinned panels, placed first: each opens its dock as the active
//!    tab, the first pin of a dock winning.
//!
//! Studio's docks are the left (explorer, assets, universe, toolbox,
//! terrain), the right (properties, history, soul, workshop) and the bottom
//! (output). A table that names another zone, or a panel these docks do not
//! hold (a tool set), is reported once and the panel goes to its home dock.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// The left dock's tabs, by their tab index.
pub const LEFT_TABS: [&str; 5] = ["explorer", "assets", "universe", "toolbox", "terrain"];
/// The right dock's tabs, by their tab index.
pub const RIGHT_TABS: [&str; 4] = ["properties", "history", "soul", "workshop"];
/// The bottom dock's panels.
pub const BOTTOM_TABS: [&str; 1] = ["output"];

/// The zones a table may name. Only left, right and bottom are docks today.
pub const ZONES: [&str; 13] = [
    "left", "right", "bottom", "top", "inner_left", "inner_right", "upper_bottom", "bottom_left", "bottom_right",
    "right_top", "right_bottom", "left_top", "left_bottom",
];

/// The docks' state: which show, their sizes, and the selected tabs. The
/// Layout menu saves and imports it, and each mode and discipline remembers
/// its own.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PanelLayout {
    pub show_explorer: bool,
    pub show_properties: bool,
    pub show_output: bool,
    pub show_toolbox: bool,
    pub show_assets: bool,
    pub show_history: bool,
    pub show_soul_panel: bool,
    pub left_panel_width: f32,
    pub right_panel_width: f32,
    pub output_height: f32,
    pub left_tab_index: i32,
    pub right_tab_index: i32,
    #[serde(default)]
    pub preset: i32,
}

/// A layout table as a mode's manifest gives it: each zone's panels in tab
/// order (in file order), sizes and active tabs by zone, and hidden panels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableView<'a> {
    pub zones: Vec<(&'a str, Vec<&'a str>)>,
    pub sizes: HashMap<&'a str, f32>,
    pub active: HashMap<&'a str, &'a str>,
    pub hidden: Vec<&'a str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dock {
    Left,
    Right,
    Bottom,
}

impl Dock {
    fn zone(self) -> &'static str {
        match self {
            Dock::Left => "left",
            Dock::Right => "right",
            Dock::Bottom => "bottom",
        }
    }
}

/// A panel's home dock and its tab index there.
fn home(panel: &str) -> Option<(Dock, usize)> {
    if let Some(i) = LEFT_TABS.iter().position(|p| *p == panel) {
        return Some((Dock::Left, i));
    }
    if let Some(i) = RIGHT_TABS.iter().position(|p| *p == panel) {
        return Some((Dock::Right, i));
    }
    BOTTOM_TABS.iter().position(|p| *p == panel).map(|i| (Dock::Bottom, i))
}

/// The key a layout is remembered under.
pub fn layout_key(mode: &str, discipline: &str) -> String {
    format!("{mode}/{discipline}")
}

/// The docks a table asks for, starting from `base` (the docks now, which
/// keep whatever the table does not set), and what could not be placed.
pub fn docks_for_table(table: &TableView, base: &PanelLayout) -> (PanelLayout, Vec<String>) {
    let mut notes = Vec::new();
    let mut lists: [Vec<(usize, &str)>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (zone, panels) in &table.zones {
        if !ZONES.contains(zone) {
            notes.push(format!("zone '{zone}' is not a zone; its panels are skipped"));
            continue;
        }
        for panel in panels {
            if table.hidden.contains(panel) {
                continue;
            }
            let Some((dock, index)) = home(panel) else {
                notes.push(format!("panel '{panel}' has no dock yet; it is skipped"));
                continue;
            };
            if dock.zone() != *zone {
                notes.push(format!("'{panel}' in '{zone}' goes to its {} dock for now", dock.zone()));
            }
            let list = &mut lists[dock as usize];
            if !list.iter().any(|(_, p)| p == panel) {
                list.push((index, *panel));
            }
        }
    }

    let mut out = base.clone();
    let active_index = |dock: Dock, list: &[(usize, &str)]| -> Option<usize> {
        let wanted = table.active.get(dock.zone()).copied();
        list.iter()
            .find(|(_, p)| Some(*p) == wanted)
            .or_else(|| list.first())
            .map(|(i, _)| *i)
    };
    match active_index(Dock::Left, &lists[0]) {
        Some(i) => {
            out.show_explorer = true;
            out.left_tab_index = i as i32;
        }
        None => {
            out.show_explorer = false;
            out.show_assets = false;
        }
    }
    match active_index(Dock::Right, &lists[1]) {
        Some(i) => {
            out.show_properties = true;
            out.right_tab_index = i as i32;
        }
        None => {
            out.show_properties = false;
            out.show_history = false;
            out.show_soul_panel = false;
        }
    }
    out.show_output = !lists[2].is_empty();

    for (zone, size) in &table.sizes {
        if !size.is_finite() || *size <= 0.0 {
            notes.push(format!("size {size} for '{zone}' is not a size"));
            continue;
        }
        match *zone {
            "left" => out.left_panel_width = size.max(MIN_LEFT_WIDTH),
            "right" => out.right_panel_width = size.max(MIN_RIGHT_WIDTH),
            "bottom" => out.output_height = size.max(MIN_OUTPUT_HEIGHT),
            other => notes.push(format!("size for '{other}' waits for its zone")),
        }
    }
    (out, notes)
}

/// The smallest dock sizes, the same as dragging allows (main.slint): a
/// narrower right panel would draw text taller than its estimate.
pub const MIN_LEFT_WIDTH: f32 = 180.0;
pub const MIN_RIGHT_WIDTH: f32 = 200.0;
pub const MIN_OUTPUT_HEIGHT: f32 = 100.0;

/// Place pinned panels first: each opens its home dock as the active tab.
/// The first pin of a dock wins its tab.
pub fn place_pins(state: &mut PanelLayout, pinned: &[String]) {
    let mut placed = [false; 3];
    for panel in pinned {
        let Some((dock, index)) = home(panel) else { continue };
        if placed[dock as usize] {
            continue;
        }
        placed[dock as usize] = true;
        match dock {
            Dock::Left => {
                state.show_explorer = true;
                state.left_tab_index = index as i32;
            }
            Dock::Right => {
                state.show_properties = true;
                state.right_tab_index = index as i32;
            }
            Dock::Bottom => state.show_output = true,
        }
    }
}

/// The docks for a mode and discipline: the user's own, else the table's
/// (else as they are), then the pins.
pub fn resolve(
    remembered: Option<&PanelLayout>,
    table: Option<&TableView>,
    now: &PanelLayout,
    pinned: &[String],
) -> (PanelLayout, Vec<String>) {
    let (mut state, notes) = match (remembered, table) {
        (Some(own), _) => (own.clone(), Vec::new()),
        (None, Some(t)) => docks_for_table(t, now),
        (None, None) => (now.clone(), Vec::new()),
    };
    place_pins(&mut state, pinned);
    (state, notes)
}

/// Toggle a panel's pin; panels with no dock cannot be pinned.
pub fn toggle_pin(pinned: &mut Vec<String>, panel: &str) -> bool {
    if home(panel).is_none() {
        return false;
    }
    if let Some(i) = pinned.iter().position(|p| p == panel) {
        pinned.remove(i);
    } else {
        pinned.push(panel.to_string());
    }
    true
}

// ============================================================================
// Engine side
// ============================================================================

/// The mode and discipline whose layout the docks show, and the layout last
/// applied for them, so a switch can tell the user's own changes from it.
#[derive(bevy::prelude::Resource, Default, Debug)]
pub struct ActiveLayoutKey {
    pub key: Option<String>,
    pub mode: String,
    pub discipline: String,
    pub applied: Option<PanelLayout>,
}

/// A mode manifest's table, borrowed in the shape [`docks_for_table`] reads.
pub fn view(table: &crate::studio_modes::LayoutTable) -> TableView<'_> {
    TableView {
        zones: table.zones.iter().map(|(z, p)| (z.as_str(), p.iter().map(String::as_str).collect())).collect(),
        sizes: table.sizes.iter().map(|(k, v)| (k.as_str(), *v)).collect(),
        active: table.active.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect(),
        hidden: table.hidden.iter().map(String::as_str).collect(),
    }
}

/// What the dock controls ask of the layout.
#[derive(Clone, Debug, PartialEq)]
pub enum LayoutRequest {
    /// Pin or unpin a panel (a dock's pin button).
    TogglePin(String),
    /// Forget the user's own layout for this mode and discipline (the
    /// Layout menu's "Reset to mode layout").
    ResetToModeLayout,
}

static REQUESTS: std::sync::Mutex<Vec<LayoutRequest>> = std::sync::Mutex::new(Vec::new());

/// Queue a request; the layout system takes it on its next frame.
pub fn request(r: LayoutRequest) {
    REQUESTS.lock().unwrap_or_else(|e| e.into_inner()).push(r);
}

/// The requests queued since the last call.
pub fn take_requests() -> Vec<LayoutRequest> {
    std::mem::take(&mut *REQUESTS.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Whether the panel a dock shows at `tab` is pinned.
pub fn tab_pinned(tabs: &[&str], tab: i32, pinned: &[String]) -> bool {
    usize::try_from(tab).ok().and_then(|i| tabs.get(i)).is_some_and(|p| pinned.iter().any(|q| q == p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> PanelLayout {
        PanelLayout {
            show_explorer: true,
            show_properties: true,
            show_output: true,
            show_toolbox: false,
            show_assets: false,
            show_history: false,
            show_soul_panel: false,
            left_panel_width: 280.0,
            right_panel_width: 420.0,
            output_height: 150.0,
            left_tab_index: 0,
            right_tab_index: 0,
            preset: 0,
        }
    }

    fn table<'a>(zones: &[(&'a str, &[&'a str])]) -> TableView<'a> {
        TableView { zones: zones.iter().map(|(z, p)| (*z, p.to_vec())).collect(), ..Default::default() }
    }

    #[test]
    fn a_table_sets_each_dock() {
        let mut t = table(&[("left", &["toolbox", "explorer"]), ("right", &["history"])]);
        t.sizes.insert("left", 320.0);
        t.sizes.insert("bottom", 10.0);
        let (s, notes) = docks_for_table(&t, &now());
        assert!(s.show_explorer);
        assert_eq!(s.left_tab_index, 3, "the first listed tab is active");
        assert_eq!(s.right_tab_index, 1);
        assert!(!s.show_output, "no bottom zone hides Output");
        assert_eq!(s.left_panel_width, 320.0);
        assert_eq!(s.output_height, MIN_OUTPUT_HEIGHT, "sizes are clamped");
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn an_active_tab_and_hidden_panels_are_honoured() {
        let mut t = table(&[("left", &["explorer", "assets", "universe"]), ("right", &["properties"])]);
        t.active.insert("left", "universe");
        t.hidden = vec!["properties"];
        let (s, _) = docks_for_table(&t, &now());
        assert_eq!(s.left_tab_index, 2);
        assert!(!s.show_properties, "a dock whose only panel is hidden closes");
    }

    #[test]
    fn what_today_cannot_place_is_reported_and_sent_home() {
        let t = table(&[("inner_left", &["properties", "toolset:water-pipe-gen"]), ("sideways", &["explorer"])]);
        let (s, notes) = docks_for_table(&t, &now());
        assert!(s.show_properties && s.right_tab_index == 0, "properties went to its home dock");
        assert!(!s.show_explorer, "the unknown zone's panels were skipped");
        assert_eq!(notes.len(), 3, "{notes:?}");
    }

    #[test]
    fn pins_come_first_and_survive_a_table() {
        let t = table(&[("left", &["explorer"]), ("right", &["properties"])]);
        let pins = vec!["history".to_string(), "soul".to_string(), "toolbox".to_string()];
        let (s, _) = resolve(None, Some(&t), &now(), &pins);
        assert_eq!(s.right_tab_index, 1, "the first right-dock pin wins its tab");
        assert_eq!(s.left_tab_index, 3);
    }

    #[test]
    fn a_remembered_layout_wins_over_the_table() {
        let mut own = now();
        own.left_panel_width = 500.0;
        own.show_output = false;
        let t = table(&[("bottom", &["output"])]);
        let (s, _) = resolve(Some(&own), Some(&t), &now(), &[]);
        assert_eq!(s, own);
        let (s, _) = resolve(None, None, &own, &[]);
        assert_eq!(s, own, "a mode with no table keeps the docks as they are");
    }

    #[test]
    fn a_remembered_layout_round_trips_through_settings() {
        let mut map = HashMap::new();
        map.insert(layout_key("civil", "water-resources"), now());
        let json = serde_json::to_string(&map).unwrap();
        let back: HashMap<String, PanelLayout> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, map);
        assert_eq!(layout_key("gaming", ""), "gaming/");
    }

    #[test]
    fn only_docked_panels_can_be_pinned() {
        let mut pins = Vec::new();
        assert!(toggle_pin(&mut pins, "history"));
        assert_eq!(pins, ["history"]);
        assert!(toggle_pin(&mut pins, "history"));
        assert!(pins.is_empty());
        assert!(!toggle_pin(&mut pins, "toolset:corpus"));
    }
}
