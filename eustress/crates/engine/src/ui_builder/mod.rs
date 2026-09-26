//! # UI Builder: a built-in Studio plugin
//!
//! A whole game UI from one line: describe the game, and the builder writes
//! a design brief, then a HUD and every screen the game needs (loading,
//! main menu, shop, quests, settings, ...), previews each one in the
//! viewport, and inserts them into the Space as ScreenGuis with the scripts
//! that run them, in one undo step.
//!
//! ```text
//! prompt ──generate / claude──▶ Blueprint ──layout──▶ Frames (nodes)
//!                                  │                    │         │
//!                         Design / Structure edit   preview   emit: StarterGui
//!                                                  (overlay)   folders + scripts
//! ```
//!
//! - [`blueprint`]: the data a UI is.
//! - [`catalog`]: styles, HUD kits, palettes, genres.
//! - [`generate`]: the offline generator, the design brief, the checklist.
//! - [`claude`]: Claude AI mode (key from Settings > Soul).
//! - [`layout`]: blueprint to GUI nodes, and the preview's pixel rects.
//! - [`emit`]: nodes to `_instance.toml` folders, the Luau scripts, and the
//!   saved blueprints.
//!
//! The panel is `ui/slint/ui_builder.slint`; all its state is the
//! `UiBuilderUi` global. Like the purchase prompt, this plugin registers its
//! own Slint callbacks and queue, so it adds nothing to `drain_slint_actions`
//! and cannot stall Studio's other clicks. It opens from the UI ribbon tab
//! and from the Plugins tab (`uibuilder:toggle`).

pub mod blueprint;
pub mod catalog;
pub mod claude;
pub mod emit;
pub mod generate;
pub mod layout;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use bevy::prelude::*;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use blueprint::{pascal_name, Anchor, Blueprint, Element, ElementKind, Item, Link, ScreenKind};
use layout::{Frame, Role};

use crate::studio_plugins::{PluginActionEvent, PluginApi, PluginCategory, PluginInfo, StudioPlugin, TabButtonSize};
use crate::ui::slint_ui::{
    GuiElementData, UbCheck, UbChip, UbExample, UbExportRow, UbField, UbKV, UbKit, UbPalette, UbRecent, UbRow, UbSection,
    UiBuilderUi,
};

/// The ribbon and Plugins-tab action that opens and closes the panel.
pub const TOGGLE_ACTION: &str = "uibuilder:toggle";

/// Where the panel sits in the viewport (main.slint places it at these
/// offsets). Studio's viewport focus test keeps clicks on it off the world.
pub const PANEL_X: f32 = 12.0;
pub const PANEL_Y: f32 = 52.0;
pub const PANEL_W: f32 = 372.0;
pub const PANEL_BOTTOM_GAP: f32 = 12.0;

// ============================================================================
// Plugin wiring
// ============================================================================

/// Must be added after the Slint UI plugin, so `SlintUiState` exists.
pub struct UiBuilderPlugin;

impl Plugin for UiBuilderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiBuilder>()
            .init_resource::<UiBuilderPreview>()
            .init_resource::<UiBuilderInbox>()
            .add_systems(
                Update,
                (register_callbacks, toggle_from_ribbon, apply_panel_messages, poll_claude, run_pending_writes, refresh_preview, sync_to_slint)
                    .chain()
                    .after(crate::ui::slint_ui::SlintSystems::Drain),
            );
    }
}

/// The Plugins-tab button. The UI ribbon tab has its own button with the
/// same action.
#[derive(Default)]
pub struct UiBuilderStudioPlugin;

impl StudioPlugin for UiBuilderStudioPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "ui-builder".to_string(),
            name: "UI Builder".to_string(),
            version: "1.0.0".to_string(),
            author: "Eustress".to_string(),
            description: "A whole game UI from one line: HUD, menus and screens, previewed live and inserted as ScreenGuis with their scripts.".to_string(),
            icon: None,
            category: PluginCategory::Building,
            permissions: Vec::new(),
        }
    }

    fn on_enable(&mut self, api: &mut PluginApi) {
        api.register_tab("plugins", "Plugins", None::<String>, 0, "ui-builder");
        api.add_tab_section("plugins", "ui_builder", "UI Builder");
        api.add_tab_button(
            "plugins",
            "ui_builder",
            "ui-builder-open",
            "UI Builder",
            Some("*"),
            "A HUD and every screen from one line, previewed in the viewport",
            TOGGLE_ACTION,
            TabButtonSize::Normal,
        );
    }
}

// ============================================================================
// State
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Selection {
    Element(usize),
    Screen(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PendingWrite {
    Insert,
    Scripts,
}

struct Inserted {
    name: String,
    at: SystemTime,
    guis: usize,
    nodes: usize,
    scripts: Vec<String>,
}

struct SavedRow {
    name: String,
    title: String,
    meta: String,
}

/// Everything the panel shows. Systems change it and bump `rev`; the sync
/// pushes it to Slint when `rev` moves.
#[derive(Resource)]
pub struct UiBuilder {
    pub open: bool,
    prompt: String,
    ai_mode: bool,
    has_key: bool,
    /// The blueprint the design brief describes, not yet generated.
    brief: Option<Blueprint>,
    brief_prompt: String,
    brief_open: Vec<bool>,
    current: Option<Blueprint>,
    history: Vec<Blueprint>,
    future: Vec<Blueprint>,
    recent: Vec<(Blueprint, SystemTime)>,
    selection: Option<Selection>,
    /// The field being typed in, so one field's keystrokes are one undo.
    editing_field: Option<(Selection, String)>,
    filter: String,
    export_name: String,
    server_script: bool,
    inserted: Option<Inserted>,
    preview_on: bool,
    /// "hud" or a screen id.
    preview_frame: String,
    job: Option<claude::AiJob>,
    toast: Option<(String, &'static str, Instant)>,
    pending: Option<PendingWrite>,
    saved: Vec<SavedRow>,
    saved_for: Option<PathBuf>,
    rev: u64,
    /// The fields card rebuilds only when this moves, so typing keeps focus.
    fields_rev: u64,
    /// The panel's own inputs (tab, prompt, name, ...) are pushed only when
    /// Rust changed them, never over what the user is typing.
    push_inputs: bool,
    tab: String,
}

impl Default for UiBuilder {
    fn default() -> Self {
        Self {
            open: false,
            prompt: String::new(),
            ai_mode: false,
            has_key: false,
            brief: None,
            brief_prompt: String::new(),
            brief_open: Vec::new(),
            current: None,
            history: Vec::new(),
            future: Vec::new(),
            recent: Vec::new(),
            selection: None,
            editing_field: None,
            filter: String::new(),
            export_name: String::new(),
            server_script: true,
            inserted: None,
            preview_on: false,
            preview_frame: "hud".to_string(),
            job: None,
            toast: None,
            pending: None,
            saved: Vec::new(),
            saved_for: None,
            rev: 1,
            fields_rev: 1,
            push_inputs: true,
            tab: "create".to_string(),
        }
    }
}

impl UiBuilder {
    fn touch(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }

    fn touch_fields(&mut self) {
        self.fields_rev = self.fields_rev.wrapping_add(1);
        self.touch();
    }

    fn toast(&mut self, text: impl Into<String>, kind: &'static str) {
        self.toast = Some((text.into(), kind, Instant::now()));
        self.touch();
    }

    /// Make `bp` the result: the previous one goes on the undo stack, the
    /// preview jumps to its HUD, and it joins Recent.
    fn apply(&mut self, bp: Blueprint) {
        if let Some(prev) = self.current.take() {
            self.history.push(prev);
            if self.history.len() > 50 {
                self.history.remove(0);
            }
        }
        self.future.clear();
        self.recent.retain(|(r, _)| !(r.name == bp.name && r.seed == bp.seed && r.source == bp.source));
        self.recent.insert(0, (bp.clone(), SystemTime::now()));
        self.recent.truncate(8);
        self.export_name = bp.name.clone();
        self.current = Some(bp);
        self.selection = None;
        self.editing_field = None;
        self.preview_on = true;
        self.preview_frame = "hud".to_string();
        self.push_inputs = true;
        self.touch_fields();
    }

    /// Change the result in place, as one undo step.
    fn edit(&mut self, f: impl FnOnce(&mut Blueprint)) {
        let Some(cur) = self.current.as_mut() else { return };
        let before = cur.clone();
        f(cur);
        if *cur != before {
            self.history.push(before);
            self.future.clear();
            self.touch();
        }
    }

    fn undo(&mut self) {
        if let Some(prev) = self.history.pop() {
            if let Some(cur) = self.current.replace(prev) {
                self.future.push(cur);
            }
            self.after_history_move();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.future.pop() {
            if let Some(cur) = self.current.replace(next) {
                self.history.push(cur);
            }
            self.after_history_move();
        }
    }

    fn after_history_move(&mut self) {
        self.editing_field = None;
        if let Some(cur) = &self.current {
            // A selection past the end of the restored lists is dropped.
            self.selection = match self.selection {
                Some(Selection::Element(i)) if i < cur.hud.len() => Some(Selection::Element(i)),
                Some(Selection::Screen(i)) if i < cur.screens.len() => Some(Selection::Screen(i)),
                _ => None,
            };
            if self.preview_frame != "hud" && cur.screen(&self.preview_frame).is_none() {
                self.preview_frame = "hud".to_string();
            }
        }
        self.touch_fields();
    }
}

/// What the panel's callbacks queue for `apply_panel_messages`.
enum PanelMsg {
    Action(String, String),
    Field(String, String),
}

#[derive(Resource, Default)]
struct UiBuilderInbox(Arc<Mutex<Vec<PanelMsg>>>);

/// The frames the Studio overlay draws while Preview is on: the HUD, plus
/// the screen being previewed with its root shown. Empty when off.
#[derive(Resource, Default, PartialEq)]
pub struct UiBuilderPreview {
    pub frames: Vec<Frame>,
}

// ============================================================================
// Systems
// ============================================================================

/// Hook the panel's two callbacks once the Slint window exists.
fn register_callbacks(slint: Option<NonSend<crate::ui::SlintUiState>>, inbox: Res<UiBuilderInbox>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let Some(slint) = slint else { return };
    let g = slint.window.global::<UiBuilderUi>();
    let q = inbox.0.clone();
    g.on_action(move |id: SharedString, arg: SharedString| {
        if let Ok(mut v) = q.lock() {
            v.push(PanelMsg::Action(id.to_string(), arg.to_string()));
        }
    });
    let q = inbox.0.clone();
    g.on_field_edited(move |key: SharedString, value: SharedString| {
        if let Ok(mut v) = q.lock() {
            v.push(PanelMsg::Field(key.to_string(), value.to_string()));
        }
    });
    *done = true;
}

/// The ribbon's UI Builder button (UI tab and Plugins tab).
fn toggle_from_ribbon(mut events: MessageReader<PluginActionEvent>, mut state: ResMut<UiBuilder>) {
    for e in events.read() {
        if e.action_id == TOGGLE_ACTION || e.action_id == "uibuilder:open" {
            state.open = !(state.open && e.action_id == TOGGLE_ACTION);
            state.touch();
        }
    }
}

fn effective_key(global: Option<&crate::soul::GlobalSoulSettings>, space: Option<&crate::soul::SoulServiceSettings>) -> String {
    match (global, space) {
        (Some(g), Some(s)) => s.effective_api_key(g),
        (Some(g), None) => g.global_api_key.trim().to_string(),
        _ => String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_panel_messages(
    inbox: Res<UiBuilderInbox>,
    mut state: ResMut<UiBuilder>,
    global_soul: Option<Res<crate::soul::GlobalSoulSettings>>,
    space_soul: Option<Res<crate::soul::SoulServiceSettings>>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
) {
    let key = effective_key(global_soul.as_deref(), space_soul.as_deref());
    let has_key = !key.is_empty();
    if state.has_key != has_key {
        state.has_key = has_key;
        state.touch();
    }
    if let Some((_, _, at)) = &state.toast {
        if at.elapsed().as_secs_f32() > 6.0 {
            state.toast = None;
            state.touch();
        }
    }
    let root = crate::space::open_space_root(space_root.as_deref());
    if state.open && state.saved_for.as_deref() != Some(root.as_path()) {
        refresh_saved(&mut state, &root);
    }

    let msgs: Vec<PanelMsg> = match inbox.0.lock() {
        Ok(mut v) if !v.is_empty() => std::mem::take(&mut *v),
        _ => return,
    };
    for msg in msgs {
        match msg {
            PanelMsg::Action(id, arg) => on_action(&mut state, &id, &arg, &key, &root),
            PanelMsg::Field(k, v) => on_field(&mut state, &k, &v),
        }
    }
}

/// A blueprint for the prompt: Claude's (in the background) when Claude
/// mode has a key, else the offline generator's, now.
fn start_generation(state: &mut UiBuilder, key: &str, seed: u64, apply: bool) {
    let prompt = state.prompt.trim().to_string();
    if prompt.is_empty() {
        state.toast("Describe your game first: one line is enough.", "warn");
        return;
    }
    if state.ai_mode && !key.is_empty() {
        state.job = Some(claude::start_blueprint(key.to_string(), &prompt, seed, apply));
        state.toast("Claude is writing the UI. It takes about a minute.", "ok");
        return;
    }
    let bp = generate::generate(&prompt, seed);
    if apply {
        let title = bp.title.clone();
        state.apply(bp);
        state.brief = None;
        state.toast(format!("Generated: {title}"), "ok");
    } else {
        set_brief(state, bp);
    }
}

fn set_brief(state: &mut UiBuilder, bp: Blueprint) {
    state.brief_prompt = bp.prompt.clone();
    state.brief_open = (0..12).map(|i| i < 2).collect();
    state.brief = Some(bp);
    state.touch();
}

fn on_action(state: &mut UiBuilder, id: &str, arg: &str, key: &str, space_root: &Path) {
    let busy = state.job.is_some();
    match id {
        "close" => {
            state.open = false;
            state.touch();
        }
        "tab" => {
            // Slint already shows the tab; remember it without pushing it back.
            state.tab = arg.to_string();
            state.touch();
        }
        "prompt" => {
            state.prompt = arg.to_string();
            state.touch();
        }
        "example" => {
            if let Some(g) = catalog::genre(arg) {
                state.prompt = g.example.to_string();
                state.push_inputs = true;
                state.touch();
            }
        }
        "mode" => {
            state.ai_mode = arg == "ai";
            if state.ai_mode && key.is_empty() {
                state.toast("Claude mode needs an API key: add one in Settings > Soul.", "warn");
            }
            state.touch();
        }
        "brief" if !busy => {
            let prompt = state.prompt.trim().to_string();
            start_generation(state, key, generate::seed_for(&prompt), false);
        }
        "generate" if !busy => {
            let prompt = state.prompt.trim().to_string();
            start_generation(state, key, generate::seed_for(&prompt), true);
        }
        "generate-from-brief" if !busy => {
            if let Some(bp) = state.brief.take() {
                let title = bp.title.clone();
                state.apply(bp);
                state.toast(format!("Generated: {title}"), "ok");
            }
        }
        "dismiss-brief" => {
            state.brief = None;
            state.touch();
        }
        "brief-section" => {
            if let Ok(i) = arg.parse::<usize>() {
                if let Some(open) = state.brief_open.get_mut(i) {
                    *open = !*open;
                    state.touch();
                }
            }
        }
        "improve" if !busy => {
            let prompt = state.prompt.trim().to_string();
            if prompt.is_empty() {
                return;
            }
            if state.ai_mode && !key.is_empty() {
                state.job = Some(claude::start_improve(key.to_string(), &prompt));
                state.touch();
            } else {
                state.prompt = improve_offline(&prompt);
                state.push_inputs = true;
                state.toast("Prompt filled out from what the offline builder would make.", "ok");
            }
        }
        "frame" => {
            state.preview_frame = arg.to_string();
            state.preview_on = true;
            state.push_inputs = true;
            state.touch();
        }
        "variation" | "regenerate" if !busy => {
            let Some(cur) = state.current.clone() else { return };
            let seed = if id == "variation" { cur.seed.wrapping_add(1).max(1) } else { cur.seed };
            let prompt = if cur.prompt.is_empty() { state.prompt.trim().to_string() } else { cur.prompt.clone() };
            if cur.source == "claude" && state.ai_mode && !key.is_empty() {
                state.job = Some(claude::start_blueprint(key.to_string(), &prompt, seed, true));
                state.toast("Claude is writing a new version.", "ok");
            } else {
                let mut bp = generate::generate(&prompt, seed);
                // A Variation of a renamed UI keeps its name.
                if cur.name != pascal_name(&cur.title) {
                    bp.name = cur.name.clone();
                }
                state.apply(bp);
                state.toast(if id == "variation" { "A new variation." } else { "Regenerated from the prompt." }, "ok");
            }
        }
        "undo" => state.undo(),
        "redo" => state.redo(),
        "restore" => {
            if let Some((bp, _)) = arg.parse::<usize>().ok().and_then(|i| state.recent.get(i)).cloned() {
                let title = bp.title.clone();
                state.apply(bp);
                state.toast(format!("Restored {title}"), "ok");
            }
        }
        "clear-recent" => {
            let current = state.current.clone();
            state.recent.retain(|(r, _)| Some(r) == current.as_ref());
            state.touch();
        }
        "style" => {
            if let Some(style) = catalog::style(arg) {
                state.edit(|bp| {
                    bp.style = style.id.to_string();
                    bp.kit = "auto".to_string();
                    if let Some(p) = catalog::palette(style.palette) {
                        bp.palette = p.to_palette();
                    }
                    bp.heading_font = style.heading_font.to_string();
                    bp.body_font = style.body_font.to_string();
                });
            }
        }
        "kit" => {
            if catalog::kit(arg).is_some() {
                state.edit(|bp| bp.kit = arg.to_string());
            }
        }
        "kit-all" => state.edit(|bp| bp.hud.iter_mut().for_each(|e| e.look.clear())),
        "palette" => {
            if let Some(p) = catalog::palette(arg) {
                state.edit(|bp| bp.palette = p.to_palette());
            }
        }
        "filter" => {
            state.filter = arg.to_string();
            state.touch();
        }
        "select" => {
            state.selection = parse_selection(arg);
            state.editing_field = None;
            if let (Some(Selection::Screen(i)), Some(cur)) = (state.selection, &state.current) {
                if let Some(s) = cur.screens.get(i) {
                    state.preview_frame = s.id.clone();
                    state.preview_on = true;
                    state.push_inputs = true;
                }
            } else if let Some(Selection::Element(_)) = state.selection {
                state.preview_frame = "hud".to_string();
                state.push_inputs = true;
            }
            state.touch_fields();
        }
        "visible" => {
            if let Some(Selection::Element(i)) = parse_selection(arg) {
                state.edit(|bp| {
                    if let Some(e) = bp.hud.get_mut(i) {
                        e.visible = !e.visible;
                    }
                });
            }
        }
        "up" | "down" => {
            let delta: isize = if id == "up" { -1 } else { 1 };
            match parse_selection(arg) {
                Some(Selection::Element(i)) => {
                    let j = i as isize + delta;
                    state.edit(|bp| {
                        if j >= 0 && (j as usize) < bp.hud.len() {
                            bp.hud.swap(i, j as usize);
                        }
                    });
                    if j >= 0 {
                        state.selection = Some(Selection::Element(j as usize));
                    }
                }
                Some(Selection::Screen(i)) => {
                    let j = i as isize + delta;
                    state.edit(|bp| {
                        if j >= 0 && (j as usize) < bp.screens.len() {
                            bp.screens.swap(i, j as usize);
                        }
                    });
                    if j >= 0 {
                        state.selection = Some(Selection::Screen(j as usize));
                    }
                }
                None => {}
            }
            state.touch_fields();
        }
        "remove" => {
            match parse_selection(arg) {
                Some(Selection::Element(i)) => state.edit(|bp| {
                    if i < bp.hud.len() {
                        bp.hud.remove(i);
                    }
                }),
                Some(Selection::Screen(i)) => state.edit(|bp| {
                    if i < bp.screens.len() {
                        let gone = bp.screens.remove(i).id;
                        // Nothing may open a screen that is gone.
                        for e in &mut bp.hud {
                            if e.opens == gone {
                                e.opens.clear();
                            }
                            e.links.retain(|l| l.opens != gone);
                        }
                        for s in &mut bp.screens {
                            for item in &mut s.items {
                                if item.opens == gone {
                                    item.opens.clear();
                                }
                            }
                        }
                    }
                }),
                None => {}
            }
            state.selection = None;
            if let Some(cur) = &state.current {
                if state.preview_frame != "hud" && cur.screen(&state.preview_frame).is_none() {
                    state.preview_frame = "hud".to_string();
                    state.push_inputs = true;
                }
            }
            state.touch_fields();
        }
        "add" => {
            let Some(kind) = ElementKind::ALL.iter().copied().find(|k| k.title() == arg) else { return };
            let mut added = None;
            state.edit(|bp| {
                let mut id = format!("hud_{}", kind.id());
                let mut n = 2;
                while bp.hud.iter().any(|e| e.id == id) {
                    id = format!("hud_{}_{n}", kind.id());
                    n += 1;
                }
                bp.hud.push(new_element(kind, &id));
                added = Some(bp.hud.len() - 1);
            });
            if let Some(i) = added {
                state.selection = Some(Selection::Element(i));
                state.preview_frame = "hud".to_string();
                state.preview_on = true;
                state.push_inputs = true;
            }
            state.touch_fields();
        }
        "export-name" => {
            state.export_name = arg.to_string();
            state.touch();
        }
        "server-script" => {
            state.server_script = arg == "on";
            state.touch();
        }
        "insert" if !busy => {
            state.pending = Some(PendingWrite::Insert);
        }
        "insert-scripts" if !busy => {
            state.pending = Some(PendingWrite::Scripts);
        }
        "load-saved" => {
            match emit::load(&emit::saved_path(space_root, arg)) {
                Ok(bp) => {
                    let title = bp.title.clone();
                    state.prompt = bp.prompt.clone();
                    state.apply(bp);
                    state.toast(format!("Loaded {title}"), "ok");
                }
                Err(e) => state.toast(e, "error"),
            }
        }
        "trash-saved" => match emit::trash_saved(space_root, arg) {
            Ok(to) => {
                refresh_saved(state, space_root);
                state.toast(format!("Moved {arg}.json to {}", to.parent().map(|p| p.display().to_string()).unwrap_or_default()), "ok");
            }
            Err(e) => state.toast(e, "error"),
        },
        "refresh-saved" => refresh_saved(state, space_root),
        "preview" => {
            state.preview_on = arg == "on";
            state.touch();
        }
        "preview-frame" => {
            // The box lists "HUD only" and then each screen's title.
            let frame = match &state.current {
                Some(cur) => cur.screens.iter().find(|s| s.title == arg).map(|s| s.id.clone()).unwrap_or_else(|| "hud".to_string()),
                None => "hud".to_string(),
            };
            state.preview_frame = frame;
            state.preview_on = true;
            state.push_inputs = true;
            state.touch();
        }
        "dismiss-toast" => {
            state.toast = None;
            state.touch();
        }
        _ => {}
    }
}

fn parse_selection(arg: &str) -> Option<Selection> {
    let (kind, index) = arg.split_once(':')?;
    let i = index.parse::<usize>().ok()?;
    match kind {
        "e" => Some(Selection::Element(i)),
        "s" => Some(Selection::Screen(i)),
        _ => None,
    }
}

/// A new element of `kind` with values that show what it does.
fn new_element(kind: ElementKind, id: &str) -> Element {
    let e = Element::new(kind, id, kind.title());
    match kind {
        ElementKind::Currency => e.value("0").icon("$"),
        ElementKind::Counter => Element { label: "SCORE".to_string(), ..e }.value("0"),
        ElementKind::Objective => Element { label: "OBJECTIVE".to_string(), ..e }.value("Reach the exit"),
        ElementKind::Meter => Element { label: "Health".to_string(), ..e }.value("100").max("100").icon("+"),
        ElementKind::Timer => Element { label: "TIME".to_string(), ..e }.value("3:00"),
        ElementKind::Hotbar => e.value("1").items(["1", "2", "3", "4", "5"]),
        ElementKind::TeamScore => Element { label: "FIRST TO 50".to_string(), ..e }.value("0-0").items(["Blue", "Red"]),
        ElementKind::Minimap => e,
        ElementKind::Button => e,
        ElementKind::Menu => Element { label: String::new(), ..e }.link("Settings", "settings", ""),
        ElementKind::Tracker => Element { label: "Quests".to_string(), ..e }.items(["First task|0/1"]),
        ElementKind::Controls => e.items(["E|Interact", "Shift|Sprint"]),
        ElementKind::Banner => Element { label: "NEW".to_string(), ..e }.value("Something happened"),
    }
}

/// The offline "Improve prompt": the prompt, then what the offline builder
/// would make of it, as words the user can edit.
fn improve_offline(prompt: &str) -> String {
    let bp = generate::generate(prompt, generate::seed_for(prompt));
    let hud: Vec<String> = bp
        .hud
        .iter()
        .filter(|e| !matches!(e.kind, ElementKind::Menu | ElementKind::Controls))
        .map(|e| if e.label.is_empty() { e.kind.title().to_lowercase() } else { e.label.to_lowercase() })
        .collect();
    let screens: Vec<String> = bp.screens.iter().filter(|s| s.kind != ScreenKind::Loading).map(|s| s.title.to_lowercase()).collect();
    let style = catalog::style(&bp.style).map(|s| s.label).unwrap_or("Arena");
    format!(
        "{}. HUD: {}. Screens: {}. {} look.",
        prompt.trim_end_matches('.'),
        hud.join(", "),
        screens.join(", "),
        style
    )
}

/// What a field edit means for the selected element or screen.
fn on_field(state: &mut UiBuilder, key: &str, value: &str) {
    let Some(sel) = state.selection else { return };
    // One undo step per field, however many keystrokes it took.
    let same_field = state.editing_field.as_ref().is_some_and(|(s, k)| *s == sel && k == key);
    let Some(cur) = state.current.as_mut() else { return };
    let before = cur.clone();
    match sel {
        Selection::Element(i) => {
            let screen_ids: Vec<String> = cur.screens.iter().map(|s| s.id.clone()).collect();
            let Some(e) = cur.hud.get_mut(i) else { return };
            match key {
                "id" => e.id = blueprint::snake_id(value),
                "label" => e.label = value.to_string(),
                "value" => e.value = value.to_string(),
                "max" => e.max = value.to_string(),
                "icon" => e.icon = value.chars().take(2).collect(),
                "digits" => e.digits = value.trim().parse::<u32>().unwrap_or(0).min(9),
                "anchor" => e.anchor = Anchor::from_id(value).unwrap_or(e.anchor),
                "items" => e.items = lines(value),
                "links" => {
                    e.links = lines(value)
                        .into_iter()
                        .map(|l| {
                            let (label, rest) = l.split_once('>').map(|(a, b)| (a.trim().to_string(), b.trim().to_string())).unwrap_or((l.trim().to_string(), String::new()));
                            let (opens, badge) = rest.split_once('[').map(|(a, b)| (a.trim().to_string(), b.trim_end_matches(']').trim().to_string())).unwrap_or((rest, String::new()));
                            let opens = blueprint::snake_id(&opens);
                            Link { label, opens: if screen_ids.contains(&opens) { opens } else { String::new() }, badge }
                        })
                        .collect()
                }
                "opens" => e.opens = if screen_ids.iter().any(|s| s == value) { value.to_string() } else { String::new() },
                "look" => e.look = catalog::KITS.iter().find(|k| k.label == value && k.id != "auto").map(|k| k.id.to_string()).unwrap_or_default(),
                _ => return,
            }
        }
        Selection::Screen(i) => {
            let Some(s) = cur.screens.get_mut(i) else { return };
            match key {
                "title" => s.title = value.to_string(),
                "kind" => s.kind = ScreenKind::from_id(value).unwrap_or(s.kind),
                "tabs" => s.tabs = lines(value),
                "items" => {
                    s.items = lines(value)
                        .into_iter()
                        .map(|l| {
                            let parts: Vec<&str> = l.split('|').map(str::trim).collect();
                            Item {
                                name: parts.first().copied().unwrap_or("").to_string(),
                                detail: parts.get(1).copied().unwrap_or("").to_string(),
                                value: parts.get(2).copied().unwrap_or("").to_string(),
                                opens: parts.get(3).map(|o| blueprint::snake_id(o)).unwrap_or_default(),
                                icon: String::new(),
                            }
                        })
                        .collect()
                }
                _ => return,
            }
        }
    }
    if *cur != before {
        if !same_field {
            state.history.push(before);
            state.future.clear();
        }
        state.editing_field = Some((sel, key.to_string()));
        // Choice boxes rebuild the card (a kind change changes the fields);
        // typed text does not, so the field keeps its focus.
        if matches!(key, "kind" | "anchor" | "look" | "opens") {
            state.touch_fields();
        } else {
            state.touch();
        }
    }
}

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

fn refresh_saved(state: &mut UiBuilder, space_root: &Path) {
    state.saved = emit::list_saved(space_root)
        .into_iter()
        .map(|(_, bp, at)| SavedRow {
            name: bp.name.clone(),
            title: bp.name.clone(),
            meta: format!("{} · {} · {}", bp.title, bp.genre, emit::ago(at)),
        })
        .collect();
    state.saved_for = Some(space_root.to_path_buf());
    state.touch();
}

/// Claude's answer, when it arrives.
fn poll_claude(mut state: ResMut<UiBuilder>, mut output: Option<ResMut<crate::ui::slint_ui::OutputConsole>>) {
    let Some(job) = state.job.as_ref() else { return };
    let Some(answer) = job.take() else { return };
    let job = state.job.take().expect("checked above");
    match (job.purpose.clone(), answer) {
        (claude::AiPurpose::ImprovePrompt, Ok(text)) => {
            state.prompt = claude::parse_improved(&text);
            state.push_inputs = true;
            state.toast("Claude filled out the prompt.", "ok");
        }
        (claude::AiPurpose::Blueprint { apply }, Ok(text)) => match claude::parse_blueprint(&text, &job.prompt, job.seed) {
            Ok(bp) => {
                let title = bp.title.clone();
                if apply {
                    state.apply(bp);
                    state.brief = None;
                } else {
                    set_brief(&mut state, bp);
                }
                state.toast(format!("Claude wrote {title} in {:.0} s.", job.started.elapsed().as_secs_f32()), "ok");
            }
            Err(e) => fall_back_offline(&mut state, &job, apply, &e, output.as_deref_mut()),
        },
        (claude::AiPurpose::Blueprint { apply }, Err(e)) => fall_back_offline(&mut state, &job, apply, &e, output.as_deref_mut()),
        (claude::AiPurpose::ImprovePrompt, Err(e)) => {
            if let Some(out) = output.as_deref_mut() {
                out.warn(format!("UI Builder: Claude could not improve the prompt: {e}"));
            }
            state.toast(format!("Claude could not improve the prompt: {}", short_error(&e)), "error");
        }
    }
    state.touch();
}

/// Claude failed: say why, and build offline so the user still gets a UI.
fn fall_back_offline(state: &mut UiBuilder, job: &claude::AiJob, apply: bool, error: &str, output: Option<&mut crate::ui::slint_ui::OutputConsole>) {
    if let Some(out) = output {
        out.warn(format!("UI Builder: Claude's blueprint failed ({error}); built offline instead."));
    }
    let bp = generate::generate(&job.prompt, job.seed);
    if apply {
        state.apply(bp);
    } else {
        set_brief(state, bp);
    }
    state.toast(format!("Claude failed ({}); built offline instead.", short_error(error)), "warn");
}

fn short_error(e: &str) -> String {
    let first = e.lines().next().unwrap_or(e);
    if first.chars().count() > 80 { format!("{}...", first.chars().take(80).collect::<String>()) } else { first.to_string() }
}

// ============================================================================
// Insert: writing into the Space
// ============================================================================

type GuiQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static eustress_common::classes::Instance,
        Option<&'static eustress_common::attributes::Tags>,
        Option<&'static crate::space::LoadedFromFile>,
        Option<&'static crate::space::instance_loader::InstanceFile>,
    ),
>;

#[allow(clippy::too_many_arguments)]
fn run_pending_writes(
    mut commands: Commands,
    mut state: ResMut<UiBuilder>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    mut registry: Option<ResMut<crate::space::SpaceFileRegistry>>,
    mut undo: Option<ResMut<crate::undo::UndoStack>>,
    mut explorer: Option<ResMut<crate::ui::slint_ui::UnifiedExplorerState>>,
    mut output: Option<ResMut<crate::ui::slint_ui::OutputConsole>>,
    guis: GuiQuery,
) {
    let Some(pending) = state.pending.take() else { return };
    let Some(mut bp) = state.current.clone() else {
        state.toast("Generate a UI first.", "warn");
        return;
    };
    let name = pascal_name(state.export_name.trim());
    let name = if state.export_name.trim().is_empty() { bp.name.clone() } else { name };
    bp.name = name.clone();
    let root = crate::space::open_space_root(space_root.as_deref());
    let frames = layout::frames(&bp);

    let result = match pending {
        PendingWrite::Insert => insert_ui(&mut commands, &bp, &frames, &root, registry.as_deref_mut(), &guis, state.server_script),
        PendingWrite::Scripts => insert_hook_scripts(&bp, &frames, &root),
    };
    match result {
        Ok(done) => {
            if let Some(stack) = undo.as_deref_mut() {
                if !done.undo.is_empty() {
                    stack.push_labeled(done.label.clone(), crate::undo::Action::Batch { actions: done.undo });
                }
            }
            if let Some(es) = explorer.as_deref_mut() {
                es.dirty = true;
                es.needs_immediate_sync = true;
            }
            if let Some(out) = output.as_deref_mut() {
                out.info(format!("UI Builder: {}", done.summary));
            }
            if pending == PendingWrite::Insert {
                if let Err(e) = emit::save(&root, &bp) {
                    if let Some(out) = output.as_deref_mut() {
                        out.warn(format!("UI Builder: the blueprint was inserted but not saved: {e}"));
                    }
                }
                state.inserted = Some(Inserted {
                    name: name.clone(),
                    at: SystemTime::now(),
                    guis: frames.len(),
                    nodes: frames.iter().map(|f| f.nodes.len()).sum(),
                    scripts: done.scripts.clone(),
                });
                // The Space's copy is now this blueprint, under this name.
                if let Some(cur) = state.current.as_mut() {
                    cur.name = name.clone();
                }
                refresh_saved(&mut state, &root);
                state.toast(format!("Inserted {name}. Press Play to try it."), "ok");
            } else {
                state.toast(done.summary.clone(), "ok");
            }
        }
        Err(e) => {
            if let Some(out) = output.as_deref_mut() {
                out.error(format!("UI Builder: {e}"));
            }
            state.toast(e, "error");
        }
    }
    state.touch();
}

struct WriteDone {
    label: String,
    summary: String,
    undo: Vec<crate::undo::Action>,
    scripts: Vec<String>,
}

/// The ScreenGuis (replacing this UI's previous copy, which goes to the
/// Space's trash), the module and start script, and optionally the server
/// hooks. One undo step for all of it.
#[allow(clippy::too_many_arguments)]
fn insert_ui(
    commands: &mut Commands,
    bp: &Blueprint,
    frames: &[Frame],
    space_root: &Path,
    mut registry: Option<&mut crate::space::SpaceFileRegistry>,
    guis: &GuiQuery,
    server_script: bool,
) -> Result<WriteDone, String> {
    let starter_dir = space_root.join("StarterGui");
    std::fs::create_dir_all(&starter_dir).map_err(|e| format!("create {}: {e}", starter_dir.display()))?;
    let gui_names: Vec<String> = frames.iter().map(|f| f.gui_name(&bp.name)).collect();
    let tag = emit::ui_tag(&bp.name);

    // The StarterGui service entity the new ScreenGuis hang under.
    let starter_entity = guis.iter().find_map(|(e, _, _, lff, _)| {
        let lff = lff?;
        (lff.service == "StarterGui" && lff.path.file_name().is_some_and(|n| n == "_service.toml")).then_some(e)
    });

    // 1. This UI's previous ScreenGuis: tagged with its name, or named like
    //    one of its frames, under StarterGui.
    let mut old: Vec<(Entity, PathBuf)> = Vec::new();
    for (e, inst, tags, lff, file) in guis.iter() {
        if inst.class_name != eustress_common::classes::ClassName::ScreenGui {
            continue;
        }
        let tagged = tags.is_some_and(|t| t.0.iter().any(|x| x == &tag));
        let named = gui_names.iter().any(|n| n == &inst.name);
        let under_starter = lff.is_some_and(|l| l.service == "StarterGui");
        if !(under_starter && (tagged || named)) {
            continue;
        }
        let folder = match (lff, file) {
            (Some(l), _) if l.path.is_dir() => l.path.clone(),
            (_, Some(f)) => f.toml_path.parent().map(Path::to_path_buf).unwrap_or_default(),
            (Some(l), None) => l.path.parent().map(Path::to_path_buf).unwrap_or_default(),
            _ => continue,
        };
        if folder.starts_with(&starter_dir) && folder != starter_dir {
            old.push((e, folder));
        }
    }

    // 2. Move them to the trash, all or none.
    let mut trashed: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (_, folder) in &old {
        if !folder.exists() {
            continue;
        }
        let to = crate::undo::Action::reserve_trash_path(space_root, folder);
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Some(reg) = registry.as_deref_mut() {
            reg.rename_in_progress.insert(folder.clone());
        }
        if let Err(e) = std::fs::rename(folder, &to) {
            // Put back what already moved, and stop before writing anything.
            for (from, t) in trashed.iter().rev() {
                let _ = std::fs::rename(t, from);
            }
            if let Some(reg) = registry.as_deref_mut() {
                reg.rename_in_progress.remove(folder);
            }
            return Err(format!(
                "could not move the previous {} to the trash ({e}). Close anything that has its files open, or pick another name in Export.",
                folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            ));
        }
        trashed.push((folder.clone(), to));
    }
    for (e, folder) in &old {
        if let Some(reg) = registry.as_deref_mut() {
            let under: Vec<PathBuf> = reg.file_to_entity.keys().filter(|p| p.starts_with(folder)).cloned().collect();
            for p in under {
                crate::space::active_db::delete_path(&p);
                reg.unregister_file(&p);
            }
            reg.rename_in_progress.remove(folder);
        }
        commands.entity(*e).despawn();
    }

    // 3. Write and spawn every node, parents first.
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut entities: Vec<Entity> = Vec::new();
    for (frame, gui_name) in frames.iter().zip(&gui_names) {
        let root_dir = starter_dir.join(gui_name);
        if root_dir.exists() {
            return Err(format!(
                "StarterGui already has a {gui_name} the builder did not make. Rename it, or pick another name in Export."
            ));
        }
        let mut dirs: Vec<PathBuf> = Vec::with_capacity(frame.nodes.len());
        let mut ents: Vec<Entity> = Vec::with_capacity(frame.nodes.len());
        for (i, node) in frame.nodes.iter().enumerate() {
            let (dir, display) = match node.parent {
                None => (root_dir.clone(), gui_name.clone()),
                Some(p) => (dirs[p].join(&node.name), node.name.clone()),
            };
            std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
            let toml_path = dir.join("_instance.toml");
            let def = emit::gui_toml(node, &display, frame.z_base + i as i32, &bp.name);
            crate::space::gui_loader::write_gui_toml(&toml_path, &def)?;
            let entity = crate::space::gui_loader::spawn_gui_element(commands, &toml_path, &def);
            let parent = match node.parent {
                None => starter_entity,
                Some(p) => Some(ents[p]),
            };
            if let Some(parent) = parent {
                commands.entity(entity).insert(ChildOf(parent));
            }
            if let Some(reg) = registry.as_deref_mut() {
                reg.register(
                    toml_path.clone(),
                    entity,
                    crate::space::FileMetadata {
                        path: dir.clone(),
                        file_type: crate::space::FileType::Directory,
                        service: "StarterGui".to_string(),
                        name: display.clone(),
                        size: 0,
                        modified: SystemTime::now(),
                        children: Vec::new(),
                    },
                );
            }
            dirs.push(dir);
            ents.push(entity);
        }
        roots.push(root_dir);
        entities.extend(ents);
    }

    // 4. The scripts that run it.
    let mut new_script_folders: Vec<PathBuf> = Vec::new();
    let mut scripts: Vec<String> = Vec::new();
    let client_dir = client_scripts_dir(space_root);
    let writes = [
        ("ReplicatedStorage".to_string(), format!("{}UI", bp.name), "module", emit::module_source(bp, frames), true, "Opens and closes the UI's screens, sets HUD values, and hands button clicks to the game. Made by the UI Builder; rewritten on every insert."),
        (client_dir.clone(), format!("{}UIStart", bp.name), "client", emit::start_source(bp), true, "Starts the UI: wires its buttons, shows the loading screen, then the main menu."),
    ];
    for (parent, script, kind, code, overwrite, summary) in writes.iter() {
        let w = write_script(space_root, parent, script, kind, code, summary, *overwrite)?;
        scripts.push(format!("{parent}/{script} ({})", w.verb()));
        if let ScriptWrite::Created(f) = w {
            new_script_folders.push(f);
        }
    }
    if server_script {
        let script = format!("{}_ServerHooks", bp.name);
        let w = write_script(space_root, "ServerScriptService", &script, "server", &emit::server_hooks_source(bp), "Server side of the UI: currencies as player attributes, and the requests its buttons send. Written once; yours to edit.", false)?;
        scripts.push(format!("ServerScriptService/{script} ({})", w.verb()));
        if let ScriptWrite::Created(f) = w {
            new_script_folders.push(f);
        }
    }

    let mut undo_actions = Vec::new();
    if !trashed.is_empty() {
        undo_actions.push(crate::undo::Action::TrashEntities { paths: trashed.clone() });
    }
    undo_actions.push(crate::undo::Action::create_entities(space_root, &roots, &entities));
    if !new_script_folders.is_empty() {
        undo_actions.push(crate::undo::Action::spawn_folders(space_root, &new_script_folders));
    }
    let node_count: usize = frames.iter().map(|f| f.nodes.len()).sum();
    Ok(WriteDone {
        label: format!("Insert UI {}", bp.name),
        summary: format!(
            "{} {} ScreenGuis ({} instances) into StarterGui{}; scripts: {}.",
            if trashed.is_empty() { "inserted" } else { "updated" },
            frames.len(),
            node_count,
            if trashed.is_empty() { String::new() } else { format!(", the previous copy is in .eustress/trash ({} folders)", trashed.len()) },
            scripts.join(", "),
        ),
        undo: undo_actions,
        scripts,
    })
}

/// The hook scripts the user owns, written only if they are not there yet.
fn insert_hook_scripts(bp: &Blueprint, frames: &[Frame], space_root: &Path) -> Result<WriteDone, String> {
    let module = space_root.join("ReplicatedStorage").join(format!("{}UI", bp.name));
    if !module.join("_instance.toml").exists() {
        return Err(format!("Insert the UI first: the hook scripts use its module, {}UI.", bp.name));
    }
    let client_dir = client_scripts_dir(space_root);
    let mut made = Vec::new();
    let mut scripts = Vec::new();
    let client = format!("{}_UIHooks", bp.name);
    let w = write_script(space_root, &client_dir, &client, "client", &emit::client_hooks_source(bp, frames), "Your code for the UI: a handler for every button. Written once; yours to edit.", false)?;
    scripts.push(format!("{client_dir}/{client} ({})", w.verb()));
    if let ScriptWrite::Created(f) = w {
        made.push(f);
    }
    let server = format!("{}_ServerHooks", bp.name);
    let w = write_script(space_root, "ServerScriptService", &server, "server", &emit::server_hooks_source(bp), "Server side of the UI: currencies as player attributes, and the requests its buttons send. Written once; yours to edit.", false)?;
    scripts.push(format!("ServerScriptService/{server} ({})", w.verb()));
    if let ScriptWrite::Created(f) = w {
        made.push(f);
    }
    let summary = if made.is_empty() {
        "Both hook scripts were already there; they are yours, so nothing was changed.".to_string()
    } else {
        format!("Scripts: {}.", scripts.join(", "))
    };
    Ok(WriteDone {
        label: format!("Insert UI scripts {}", bp.name),
        summary,
        undo: if made.is_empty() { Vec::new() } else { vec![crate::undo::Action::spawn_folders(space_root, &made)] },
        scripts,
    })
}

/// Where client scripts go in this Space: a top-level StarterPlayerScripts
/// when it has one, else StarterPlayer's.
fn client_scripts_dir(space_root: &Path) -> String {
    if space_root.join("StarterPlayerScripts").is_dir() {
        "StarterPlayerScripts".to_string()
    } else {
        "StarterPlayer/StarterPlayerScripts".to_string()
    }
}

enum ScriptWrite {
    Created(PathBuf),
    Updated,
    Kept,
}

impl ScriptWrite {
    fn verb(&self) -> &'static str {
        match self {
            ScriptWrite::Created(_) => "new",
            ScriptWrite::Updated => "updated",
            ScriptWrite::Kept => "kept",
        }
    }
}

/// A script through the `create_script` tool (the SoulScript folder every
/// script has), or, when the folder is already there, its source rewritten
/// in place (`overwrite`) or left alone.
#[allow(clippy::too_many_arguments)]
fn write_script(space_root: &Path, parent: &str, name: &str, kind: &str, code: &str, summary: &str, overwrite: bool) -> Result<ScriptWrite, String> {
    let folder = space_root.join(parent).join(name);
    let toml_path = folder.join("_instance.toml");
    if toml_path.exists() {
        if !overwrite {
            return Ok(ScriptWrite::Kept);
        }
        let source = std::fs::read_to_string(&toml_path)
            .ok()
            .and_then(|t| t.parse::<toml::Value>().ok())
            .and_then(|doc| doc.get("script").and_then(|s| s.get("source")).and_then(|s| s.as_str()).map(str::to_string))
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("{parent}/{name} exists but names no source file; rename it or remove it"))?;
        let path = folder.join(source);
        crate::space::gui_loader::write_atomic(&path, code.as_bytes()).map_err(|e| format!("write {}: {e}", path.display()))?;
        return Ok(ScriptWrite::Updated);
    }
    use eustress_tools::registry::ToolHandler;
    let ctx = eustress_tools::registry::ToolContext {
        space_root: space_root.to_path_buf(),
        universe_root: space_root.parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_else(|| space_root.to_path_buf()),
        user_id: None,
        username: None,
        luau_executor: None,
        display_unit: None,
        cancelled: None,
        permissions: eustress_tools::Permissions::standard().for_principal("ui-builder"),
    };
    let result = eustress_tools::universe_tools::CreateScriptTool.execute(
        serde_json::json!({ "name": name, "code": code, "language": "luau", "kind": kind, "parent": parent, "summary": summary }),
        &ctx,
    );
    if !result.success {
        return Err(format!("{parent}/{name}: {}", result.content));
    }
    let made = result
        .structured_data
        .as_ref()
        .and_then(|d| d.get("folder"))
        .and_then(|f| f.as_str())
        .map(PathBuf::from)
        .unwrap_or(folder);
    Ok(ScriptWrite::Created(made))
}

// ============================================================================
// Preview
// ============================================================================

fn play_stopped(play: &Option<Res<State<crate::play_mode::PlayModeState>>>) -> bool {
    play.as_ref().map_or(true, |s| *s.get() == crate::play_mode::PlayModeState::Editing)
}

/// Rebuild the preview's frames when the blueprint or the previewed frame
/// changes. Only an actual change touches the resource, which is what
/// wakes the overlay sync.
fn refresh_preview(
    state: Res<UiBuilder>,
    mut preview: ResMut<UiBuilderPreview>,
    play: Option<Res<State<crate::play_mode::PlayModeState>>>,
    mut last: Local<(u64, bool)>,
) {
    let active = state.open && state.preview_on && state.current.is_some() && play_stopped(&play);
    if *last == (state.rev, active) {
        return;
    }
    *last = (state.rev, active);
    let mut frames = Vec::new();
    if active {
        if let Some(bp) = &state.current {
            frames.push(layout::hud_frame(bp));
            if let Some(s) = bp.screen(&state.preview_frame) {
                let mut f = layout::screen_frame(bp, s);
                // The screen shows in the preview as it does once opened.
                if let Some(root) = f.nodes.iter_mut().find(|n| n.role == Role::Root) {
                    root.visible = true;
                }
                frames.push(f);
            }
        }
    }
    let next = UiBuilderPreview { frames };
    if *preview.bypass_change_detection() != next {
        *preview = next;
    }
}

/// The preview as overlay rows, resolved in the viewport's logical pixels.
/// Called by the Studio overlay sync (`sync_gui_elements_to_slint`), after
/// the Space's own ScreenGuis, so the preview draws on top.
pub fn preview_rows(preview: &UiBuilderPreview, viewport: (f32, f32)) -> Vec<GuiElementData> {
    let mut out = Vec::new();
    for frame in &preview.frames {
        let rects = layout::resolve(&frame.nodes, viewport);
        let shown = layout::shown(&frame.nodes);
        for (i, n) in frame.nodes.iter().enumerate() {
            if n.class == "ScreenGui" || !shown[i] {
                continue;
            }
            let (x, y, w, h) = rects[i];
            out.push(GuiElementData {
                x,
                y,
                width: w,
                height: h,
                z_order: frame.z_base + i as i32,
                visible: true,
                clip_children: false,
                scroll_x: 0.0,
                scroll_y: 0.0,
                bg_r: n.bg[0],
                bg_g: n.bg[1],
                bg_b: n.bg[2],
                bg_a: n.bg[3],
                border_size: n.border,
                border_r: n.border_color[0],
                border_g: n.border_color[1],
                border_b: n.border_color[2],
                border_a: n.border_color[3],
                corner_radius: n.corner,
                text: n.text.as_str().into(),
                text_r: n.text_color[0],
                text_g: n.text_color[1],
                text_b: n.text_color[2],
                text_a: n.text_color[3],
                font_size: n.font_size,
                font_weight: crate::space::gui_loader::font_weight_from_name(&n.font),
                text_align: n.align.to_ascii_lowercase().into(),
                text_y_align: "center".into(),
                image_source: slint::Image::default(),
                has_image: false,
                class_type: n.class.to_ascii_lowercase().into(),
            });
        }
    }
    out
}

/// Whether a cursor at `pos` (logical pixels) is over the open panel, given
/// the viewport rect `(x, y, w, h)`: Studio's viewport focus test treats the
/// panel as UI, not world.
pub fn panel_contains(window: &crate::ui::slint_ui::StudioWindow, pos: (f32, f32), viewport: (f32, f32, f32, f32)) -> bool {
    let open = window.global::<UiBuilderUi>().get_open();
    if !open || window.get_play_state().as_str() != "stopped" {
        return false;
    }
    let (vx, vy, _vw, vh) = viewport;
    let x0 = vx + PANEL_X;
    let y0 = vy + PANEL_Y;
    let y1 = vy + vh - PANEL_BOTTOM_GAP;
    pos.0 >= x0 && pos.0 <= x0 + PANEL_W && pos.1 >= y0 && pos.1 <= y1
}

// ============================================================================
// Slint sync
// ============================================================================

fn s(text: impl AsRef<str>) -> SharedString {
    SharedString::from(text.as_ref())
}

fn color(c: [u8; 3]) -> slint::Color {
    slint::Color::from_rgb_u8(c[0], c[1], c[2])
}

fn color_f(c: [f32; 4]) -> slint::Color {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    slint::Color::from_argb_u8(b(c[3]), b(c[0]), b(c[1]), b(c[2]))
}

/// Update a list property row by row: replacing the model would rebuild
/// every row, resetting hover and dropping a click in flight.
fn push_rows<T: Clone + PartialEq + 'static>(current: ModelRc<T>, rows: Vec<T>, set: impl FnOnce(ModelRc<T>)) {
    if let Some(live) = current.as_any().downcast_ref::<VecModel<T>>() {
        let old = live.row_count();
        for (i, r) in rows.iter().enumerate() {
            if i < old {
                if live.row_data(i).as_ref() != Some(r) {
                    live.set_row_data(i, r.clone());
                }
            } else {
                live.push(r.clone());
            }
        }
        for i in (rows.len()..old).rev() {
            live.remove(i);
        }
    } else {
        set(ModelRc::new(VecModel::from(rows)));
    }
}

fn strings(v: Vec<String>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(v.into_iter().map(SharedString::from).collect::<Vec<_>>()))
}

/// Push the state to the panel when it changed.
fn sync_to_slint(
    slint: Option<NonSend<crate::ui::SlintUiState>>,
    mut state: ResMut<UiBuilder>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    mut last: Local<(u64, u64)>,
) {
    let Some(slint) = slint else { return };
    let g = slint.window.global::<UiBuilderUi>();

    // The ribbon toggles `open` in Rust; the panel's own close goes through
    // an action, so Rust always owns it.
    if g.get_open() != state.open {
        g.set_open(state.open);
    }
    if last.0 == state.rev && last.1 == state.fields_rev {
        return;
    }
    let state: &mut UiBuilder = &mut state;
    let root = crate::space::open_space_root(space_root.as_deref());

    if std::mem::take(&mut state.push_inputs) {
        g.set_tab(s(&state.tab));
        g.set_prompt(s(&state.prompt));
        g.set_export_name(s(&state.export_name));
        g.set_server_script(state.server_script);
        g.set_preview(state.preview_on);
        g.set_filter(s(&state.filter));
        let index = match &state.current {
            Some(cur) => cur.screens.iter().position(|x| x.id == state.preview_frame).map_or(0, |i| i as i32 + 1),
            None => 0,
        };
        g.set_preview_index(index);
    }

    g.set_ai_mode(state.ai_mode);
    g.set_has_key(state.has_key);
    g.set_busy(state.job.is_some());
    g.set_status(s(match (&state.job, state.ai_mode && state.has_key) {
        (Some(_), _) => "Claude is writing...",
        (None, true) => "Claude",
        (None, false) => "Offline",
    }));
    push_rows(
        g.get_examples(),
        catalog::GENRES
            .iter()
            .map(|x| UbExample { id: s(x.id), title: s(x.example_title), detail: s(x.example), icon: s(x.icon) })
            .collect(),
        |m| g.set_examples(m),
    );
    let (toast, kind) = state.toast.as_ref().map(|(t, k, _)| (t.clone(), *k)).unwrap_or_default();
    g.set_toast(s(toast));
    g.set_toast_kind(s(if kind.is_empty() { "ok" } else { kind }));

    sync_brief(&g, state);
    sync_result(&g, state);
    sync_design(&g, state);
    sync_structure(&g, state, last.1 != state.fields_rev);
    sync_export(&g, state, &root);

    *last = (state.rev, state.fields_rev);
}

fn sync_brief(g: &UiBuilderUi, state: &UiBuilder) {
    let Some(bp) = &state.brief else {
        g.set_has_brief(false);
        return;
    };
    g.set_has_brief(true);
    g.set_brief_meta(s(format!("Written {} · 12 sections", if bp.source == "claude" { "by Claude" } else { "offline" })));
    g.set_brief_stale(state.prompt.trim() != state.brief_prompt.trim());
    let style = catalog::style(&bp.style);
    let kit = generate::resolved_kit(bp);
    push_rows(
        g.get_glance(),
        vec![
            UbKV { key: s("Look"), value: s(format!("{} · {} kit", style.map(|x| x.label).unwrap_or("Custom"), kit.label)) },
            UbKV { key: s("Palette"), value: s(&bp.palette.label) },
            UbKV { key: s("Fonts"), value: s(format!("{} / {}", bp.heading_font, bp.body_font)) },
            UbKV { key: s("Logo"), value: s(format!("{} · Stacked", bp.title.to_uppercase())) },
            UbKV { key: s("Frames"), value: s(format!("HUD + {} screens", bp.screens.len())) },
        ],
        |m| g.set_glance(m),
    );
    let p = &bp.palette;
    push_rows(g.get_glance_swatches(), [p.primary, p.secondary, p.accent, p.dark, p.light].into_iter().map(color).collect(), |m| {
        g.set_glance_swatches(m)
    });
    let sections = generate::brief(bp);
    push_rows(
        g.get_brief(),
        sections
            .iter()
            .enumerate()
            .map(|(i, sec)| UbSection {
                index: i as i32,
                title: s(sec.title.to_uppercase()),
                body: s(&sec.body),
                open: state.brief_open.get(i).copied().unwrap_or(false),
            })
            .collect(),
        |m| g.set_brief(m),
    );
}

fn sync_result(g: &UiBuilderUi, state: &UiBuilder) {
    g.set_can_undo(!state.history.is_empty());
    g.set_can_redo(!state.future.is_empty());
    push_rows(
        g.get_recent(),
        state
            .recent
            .iter()
            .enumerate()
            .map(|(i, (bp, at))| UbRecent {
                id: s(i.to_string()),
                title: s(&bp.title),
                meta: s(format!("{} · {} screens · {}", bp.genre, bp.screens.len(), emit::ago(*at))),
                icon: s(catalog::genre(&bp.genre).map(|g| g.icon).unwrap_or("sparkle")),
                current: state.current.as_ref().is_some_and(|c| c.name == bp.name && c.seed == bp.seed && c.source == bp.source),
            })
            .collect(),
        |m| g.set_recent(m),
    );
    let Some(bp) = &state.current else {
        g.set_has_result(false);
        g.set_preview_frames(strings(vec!["HUD only".to_string()]));
        return;
    };
    g.set_has_result(true);
    g.set_result_title(s(&bp.title));
    g.set_result_meta(s(format!(
        "{} · {} HUD elements · {} screens · seed {}",
        bp.genre,
        bp.hud.iter().filter(|e| e.visible).count(),
        bp.screens.len(),
        bp.seed
    )));
    g.set_result_source(s(if bp.source == "claude" { "Claude" } else { "Offline" }));
    let mut chips = vec![UbChip { id: s("hud"), label: s("HUD"), icon: s("screen"), selected: state.preview_on && state.preview_frame == "hud" }];
    chips.extend(bp.screens.iter().map(|x| UbChip {
        id: s(&x.id),
        label: s(&x.title),
        icon: s(x.kind.icon()),
        selected: state.preview_on && state.preview_frame == x.id,
    }));
    push_rows(g.get_frames(), chips, |m| g.set_frames(m));
    let mut names = vec!["HUD only".to_string()];
    names.extend(bp.screens.iter().map(|x| x.title.clone()));
    push_rows(g.get_preview_frames(), names.into_iter().map(SharedString::from).collect(), |m| g.set_preview_frames(m));
}

fn sync_design(g: &UiBuilderUi, state: &UiBuilder) {
    let default_bp = Blueprint::default();
    let bp = state.current.as_ref().unwrap_or(&default_bp);
    let has = state.current.is_some();
    push_rows(
        g.get_styles(),
        catalog::STYLES.iter().map(|x| UbChip { id: s(x.id), label: s(x.label), icon: s(""), selected: has && bp.style == x.id }).collect(),
        |m| g.set_styles(m),
    );
    g.set_style_about(s(catalog::style(&bp.style).map(|x| x.about).unwrap_or("")));
    let resolved = generate::resolved_kit(bp);
    push_rows(
        g.get_kits(),
        catalog::KITS
            .iter()
            .map(|k| {
                let shown = if k.id == "auto" { resolved } else { k };
                let (fill, ink, line, line_px) = layout::kit_swatch(&bp.palette, shown);
                UbKit {
                    id: s(k.id),
                    label: s(k.label),
                    selected: has && (bp.kit == k.id || (k.id == "auto" && (bp.kit.is_empty() || bp.kit == "auto"))),
                    plate: color_f(fill),
                    ink: color_f(ink),
                    line: color_f(line),
                    line_width: line_px,
                    radius: shown.radius.min(11.0),
                    shadow: shown.shadow,
                }
            })
            .collect(),
        |m| g.set_kits(m),
    );
    g.set_kit_about(s(format!("{}: {}", resolved.label, resolved.about)));
    g.set_kit_overrides(bp.hud.iter().filter(|e| !e.look.is_empty()).count() as i32);
    push_rows(
        g.get_palettes(),
        catalog::PALETTES
            .iter()
            .map(|p| UbPalette {
                id: s(p.id),
                label: s(p.label),
                selected: has && bp.palette.id == p.id,
                c1: color(p.primary),
                c2: color(p.secondary),
                c3: color(p.accent),
                c4: color(p.dark),
                c5: color(p.light),
            })
            .collect(),
        |m| g.set_palettes(m),
    );
    let p = &bp.palette;
    push_rows(g.get_current_swatches(), [p.primary, p.secondary, p.accent, p.dark, p.light].into_iter().map(color).collect(), |m| {
        g.set_current_swatches(m)
    });
}

fn sync_structure(g: &UiBuilderUi, state: &UiBuilder, fields_changed: bool) {
    let Some(bp) = &state.current else {
        push_rows(g.get_elements(), Vec::new(), |m| g.set_elements(m));
        push_rows(g.get_screens(), Vec::new(), |m| g.set_screens(m));
        if fields_changed {
            g.set_fields(ModelRc::new(VecModel::from(Vec::<UbField>::new())));
        }
        return;
    };
    let filter = state.filter.trim().to_lowercase();
    let keep = |title: &str, detail: &str| filter.is_empty() || title.to_lowercase().contains(&filter) || detail.to_lowercase().contains(&filter);
    let elements: Vec<UbRow> = bp
        .hud
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let title = e.display_title();
            let mut detail = e.anchor.id().to_string();
            if matches!(e.kind, ElementKind::Hotbar) {
                detail.push_str(&format!(" · {} slots", e.items.len()));
            }
            keep(&title, &detail).then(|| UbRow {
                id: s(format!("e:{i}")),
                title: s(title),
                detail: s(detail),
                icon: s(e.kind.icon()),
                visible: e.visible,
                selected: state.selection == Some(Selection::Element(i)),
            })
        })
        .collect();
    push_rows(g.get_elements(), elements, |m| g.set_elements(m));
    let screens: Vec<UbRow> = bp
        .screens
        .iter()
        .enumerate()
        .filter_map(|(i, x)| {
            let detail = format!("{} · {} items", x.kind.id().replace('_', " "), x.items.len());
            keep(&x.title, &detail).then(|| UbRow {
                id: s(format!("s:{i}")),
                title: s(&x.title),
                detail: s(detail),
                icon: s(x.kind.icon()),
                visible: true,
                selected: state.selection == Some(Selection::Screen(i)),
            })
        })
        .collect();
    push_rows(g.get_screens(), screens, |m| g.set_screens(m));
    push_rows(
        g.get_add_kinds(),
        ElementKind::ALL.iter().map(|k| SharedString::from(k.title())).collect(),
        |m| g.set_add_kinds(m),
    );

    if !fields_changed {
        return;
    }
    let (title, fields) = match state.selection {
        Some(Selection::Element(i)) => match bp.hud.get(i) {
            Some(e) => (e.display_title(), element_fields(bp, e)),
            None => (String::new(), Vec::new()),
        },
        Some(Selection::Screen(i)) => match bp.screens.get(i) {
            Some(x) => (format!("Screen: {}", x.title), screen_fields(x)),
            None => (String::new(), Vec::new()),
        },
        None => (String::new(), Vec::new()),
    };
    g.set_selected_title(s(title));
    // Replaced, not diffed: this only runs when the selection or a choice
    // box changed, never while a field is being typed in.
    g.set_fields(ModelRc::new(VecModel::from(fields)));
}

fn field(key: &str, label: &str, value: impl Into<String>, hint: &str) -> UbField {
    UbField { key: s(key), label: s(label), value: s(value.into()), hint: s(hint), options: strings(Vec::new()), multiline: false }
}

fn choice(key: &str, label: &str, value: impl Into<String>, hint: &str, options: Vec<String>) -> UbField {
    UbField { options: strings(options), ..field(key, label, value, hint) }
}

fn area(key: &str, label: &str, value: impl Into<String>, hint: &str) -> UbField {
    UbField { multiline: true, ..field(key, label, value, hint) }
}

fn element_fields(bp: &Blueprint, e: &Element) -> Vec<UbField> {
    let anchors = Anchor::ALL.iter().map(|a| a.id().to_string()).collect();
    let mut screens = vec!["(none)".to_string()];
    screens.extend(bp.screens.iter().map(|x| x.id.clone()));
    let mut looks = vec!["Kit's choice".to_string()];
    looks.extend(catalog::KITS.iter().filter(|k| k.id != "auto").map(|k| k.label.to_string()));
    let look = catalog::kit(&e.look).map(|k| k.label.to_string()).unwrap_or_else(|| "Kit's choice".to_string());

    let mut f = vec![
        choice("anchor", "Anchor", e.anchor.id(), "", anchors),
        field("id", "Id", &e.id, &format!("Scripts set it with UI.set(\"{}\", value).", e.id)),
    ];
    match e.kind {
        ElementKind::Menu => {
            let text = e.links.iter().map(|l| {
                let mut line = format!("{} > {}", l.label, l.opens);
                if !l.badge.is_empty() {
                    line.push_str(&format!(" [{}]", l.badge));
                }
                line
            });
            f.push(area("links", "Buttons", text.collect::<Vec<_>>().join("\n"), "One per line: Label > screen_id [badge]. Leave the screen empty for a button your game handles."));
        }
        _ => f.push(field("label", "Label", &e.label, match e.kind {
            ElementKind::Counter => "The small caption: DOOR, PAGES, LEVEL, NIGHT.",
            ElementKind::Currency => "The currency's name; the server script keeps a player attribute of this name.",
            _ => "",
        })),
    }
    match e.kind {
        ElementKind::Counter => {
            f.push(field("value", "Value", &e.value, "Set it from your game with UI.set."));
            f.push(field("max", "Goal", &e.max, "Adds /goal: PAGES 3/8. Leave empty for none."));
            f.push(field("digits", "Digits", e.digits.to_string(), "Zero-pads the number: 4 shows 0042."));
        }
        ElementKind::Meter => {
            f.push(field("value", "Value", &e.value, ""));
            f.push(field("max", "Max", &e.max, "The bar is value / max."));
            f.push(field("icon", "Icon", &e.icon, "One or two characters in the badge."));
        }
        ElementKind::Currency => {
            f.push(field("value", "Value", &e.value, "The starting balance."));
            f.push(field("icon", "Icon", &e.icon, "One or two characters in the coin."));
            f.push(choice("opens", "+ opens", if e.opens.is_empty() { "(none)".to_string() } else { e.opens.clone() }, "A + button beside the amount, opening this screen.", screens.clone()));
        }
        ElementKind::Objective | ElementKind::Timer | ElementKind::Banner => {
            f.push(field("value", "Value", &e.value, ""));
        }
        ElementKind::Button => {
            f.push(choice("opens", "Opens", if e.opens.is_empty() { "(none)".to_string() } else { e.opens.clone() }, "Or (none) for a button your game handles.", screens.clone()));
        }
        ElementKind::Hotbar => {
            f.push(field("value", "Selected", &e.value, "The selected slot, from 1."));
            f.push(area("items", "Slots", e.items.join("\n"), "One slot per line."));
        }
        ElementKind::TeamScore => {
            f.push(field("value", "Score", &e.value, "Left-right, like 39-45."));
            f.push(area("items", "Teams", e.items.join("\n"), "The left team, then the right."));
        }
        ElementKind::Tracker => f.push(area("items", "Lines", e.items.join("\n"), "One per line: text|progress, like Collect 20 batteries|20/20.")),
        ElementKind::Controls => f.push(area("items", "Keys", e.items.join("\n"), "One per line: key|action, like Tab|Scoreboard.")),
        ElementKind::Minimap | ElementKind::Menu => {}
    }
    f.push(choice("look", "Look", look, "Keep this element's own kit when the HUD kit changes.", looks));
    f
}

fn screen_fields(x: &blueprint::Screen) -> Vec<UbField> {
    let kinds = ScreenKind::ALL.iter().map(|k| k.id().to_string()).collect();
    let items = x
        .items
        .iter()
        .map(|i| {
            let mut parts = vec![i.name.clone(), i.detail.clone(), i.value.clone()];
            if !i.opens.is_empty() {
                parts.push(i.opens.clone());
            }
            while parts.len() > 1 && parts.last().is_some_and(|p| p.is_empty()) {
                parts.pop();
            }
            parts.join(" | ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let hint = match x.kind {
        ScreenKind::Shop => "One per line: name | detail | price. R$ prices are Robux products.",
        ScreenKind::MainMenu => "One button per line: label | | | screen_id. The first is the main action.",
        ScreenKind::List => "One per line: name | detail | progress (3/8).",
        ScreenKind::Settings => "One per line: name | detail | on or off.",
        ScreenKind::Results | ScreenKind::Leaderboard => "One per line: name | detail | value.",
        _ => "One per line: name | detail | value.",
    };
    let mut f = vec![
        field("title", "Title", &x.title, &format!("Scripts open it with UI.open(\"{}\").", x.id)),
        choice("kind", "Layout", x.kind.id(), "", kinds),
    ];
    if matches!(x.kind, ScreenKind::Shop | ScreenKind::Settings | ScreenKind::MainMenu) {
        f.push(area("tabs", if x.kind == ScreenKind::MainMenu { "Offer" } else { "Tabs" }, x.tabs.join("\n"), if x.kind == ScreenKind::MainMenu { "A featured offer's name, or empty." } else { "One tab per line." }));
    }
    f.push(area("items", "Items", items, hint));
    f
}

fn sync_export(g: &UiBuilderUi, state: &UiBuilder, root: &Path) {
    let Some(cur) = &state.current else {
        push_rows(g.get_saved(), saved_rows(state), |m| g.set_saved(m));
        g.set_saved_meta(s(saved_meta(state)));
        g.set_usage(s(""));
        return;
    };
    let name = if state.export_name.trim().is_empty() { cur.name.clone() } else { pascal_name(state.export_name.trim()) };
    let client_dir = client_scripts_dir(root);
    let exists = |rel: &str| root.join(rel).exists();
    let hud_there = exists(&format!("StarterGui/{name}_HUD"));
    let badge = |there: bool, keeps: bool| s(if !there { "New" } else if keeps { "Kept" } else { "Update" });

    let mut rows = vec![UbExportRow {
        title: s(format!("Your UI: {name}_HUD and {} screens", cur.screens.len())),
        path: s(format!("StarterGui.{name}_*")),
        badge: badge(hud_there, false),
    }];
    rows.push(UbExportRow {
        title: s(format!("{name}UI (module)")),
        path: s(format!("ReplicatedStorage.{name}UI")),
        badge: badge(exists(&format!("ReplicatedStorage/{name}UI")), false),
    });
    rows.push(UbExportRow {
        title: s(format!("{name}UIStart (client script)")),
        path: s(format!("{}.{name}UIStart", client_dir.replace('/', "."))),
        badge: badge(exists(&format!("{client_dir}/{name}UIStart")), false),
    });
    if state.server_script {
        rows.push(UbExportRow {
            title: s(format!("{name}_ServerHooks (server script)")),
            path: s(format!("ServerScriptService.{name}_ServerHooks")),
            badge: badge(exists(&format!("ServerScriptService/{name}_ServerHooks")), true),
        });
    }
    rows.push(UbExportRow {
        title: s("Blueprint (to load it again)"),
        path: s(format!(".eustress/ui_builder/{name}.json")),
        badge: badge(emit::saved_path(root, &name).exists(), false),
    });
    push_rows(g.get_export_rows(), rows, |m| g.set_export_rows(m));
    g.set_insert_label(s(if hud_there { format!("Update {name}") } else { "Insert into game".to_string() }));

    match &state.inserted {
        Some(ins) if ins.name == name => {
            g.set_inserted(true);
            g.set_inserted_title(s(format!("Inserted {} · {}", ins.name, emit::ago(ins.at))));
            g.set_inserted_meta(s(format!(
                "{} ScreenGuis, {} instances in StarterGui. {}. Press Play (F5) to try it; Ctrl+Z takes it all out.",
                ins.guis,
                ins.nodes,
                ins.scripts.join(", ")
            )));
        }
        _ => g.set_inserted(false),
    }

    let frames = layout::frames(cur);
    let checks = generate::checklist(cur);
    g.set_todo_count(checks.iter().filter(|c| c.level != "info").map(|c| c.count as i32).sum());
    g.set_note_count(checks.iter().filter(|c| c.level == "info").map(|c| c.count as i32).sum());
    push_rows(
        g.get_checklist(),
        checks
            .into_iter()
            .map(|c| UbCheck { label: s(c.label), count: c.count as i32, level: s(c.level), detail: s(c.detail) })
            .collect(),
        |m| g.set_checklist(m),
    );
    let handlers = frames
        .iter()
        .flat_map(|f| f.nodes.iter())
        .filter_map(|n| match &n.role {
            Role::Action { action, .. } => Some(action.clone()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    push_rows(
        g.get_script_rows(),
        vec![
            UbExportRow {
                title: s(format!("UI hooks · {handlers} handlers")),
                path: s(format!("{}.{name}_UIHooks", client_dir.replace('/', "."))),
                badge: badge(exists(&format!("{client_dir}/{name}_UIHooks")), true),
            },
            UbExportRow {
                title: s("Server hooks · currencies and requests"),
                path: s(format!("ServerScriptService.{name}_ServerHooks")),
                badge: badge(exists(&format!("ServerScriptService/{name}_ServerHooks")), true),
            },
        ],
        |m| g.set_script_rows(m),
    );
    push_rows(g.get_saved(), saved_rows(state), |m| g.set_saved(m));
    g.set_saved_meta(s(saved_meta(state)));
    let first_value = cur
        .hud
        .iter()
        .find(|e| !matches!(e.kind, ElementKind::Button | ElementKind::Menu | ElementKind::Controls | ElementKind::Minimap))
        .map(|e| e.id.as_str())
        .unwrap_or("hud_value");
    let first_screen = cur.screens.iter().find(|x| !x.kind.is_fullscreen()).map(|x| x.id.as_str()).unwrap_or("settings");
    g.set_usage(s(format!(
        "local UI = require(game.ReplicatedStorage:WaitForChild(\"{name}UI\"))\nUI.set(\"{first_value}\", 42)\nUI.open(\"{first_screen}\")\nUI.on(\"buy\", function(item) ... end)"
    )));
}

fn saved_rows(state: &UiBuilder) -> Vec<UbRecent> {
    let current = state.current.as_ref().map(|c| if state.export_name.trim().is_empty() { c.name.clone() } else { pascal_name(state.export_name.trim()) });
    state
        .saved
        .iter()
        .map(|r| UbRecent { id: s(&r.name), title: s(&r.title), meta: s(&r.meta), icon: s("document"), current: current.as_deref() == Some(r.name.as_str()) })
        .collect()
}

fn saved_meta(state: &UiBuilder) -> String {
    match state.saved.len() {
        0 => "Nothing saved yet: Insert saves the blueprint here.".to_string(),
        1 => "1 blueprint".to_string(),
        n => format!("{n} blueprints"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_are_undoable_and_a_field_is_one_step() {
        let mut st = UiBuilder::default();
        st.apply(generate::generate("a horror game", 1234));
        st.selection = Some(Selection::Element(0));
        on_field(&mut st, "label", "R");
        on_field(&mut st, "label", "RO");
        on_field(&mut st, "label", "ROOM!");
        assert_eq!(st.current.as_ref().unwrap().hud[0].label, "ROOM!");
        st.undo();
        assert_eq!(st.current.as_ref().unwrap().hud[0].label, "ROOM");
        st.redo();
        assert_eq!(st.current.as_ref().unwrap().hud[0].label, "ROOM!");
    }

    #[test]
    fn removing_a_screen_drops_every_way_in() {
        let mut st = UiBuilder::default();
        st.apply(generate::generate("a tycoon", 99));
        let i = st.current.as_ref().unwrap().screens.iter().position(|x| x.id == "shop").unwrap();
        on_action(&mut st, "remove", &format!("s:{i}"), "", Path::new("."));
        let bp = st.current.as_ref().unwrap();
        assert!(bp.screen("shop").is_none());
        assert!(!bp.links().iter().any(|(_, to)| to == "shop"));
    }

    #[test]
    fn menu_links_parse_from_lines() {
        let mut st = UiBuilder::default();
        st.apply(generate::generate("a tycoon", 99));
        let i = st.current.as_ref().unwrap().hud.iter().position(|e| e.kind == ElementKind::Menu).unwrap();
        st.selection = Some(Selection::Element(i));
        on_field(&mut st, "links", "Shop > shop [3]\nNowhere > missing\nPlay");
        let links = &st.current.as_ref().unwrap().hud[i].links;
        assert_eq!(links.len(), 3);
        assert_eq!((links[0].opens.as_str(), links[0].badge.as_str()), ("shop", "3"));
        assert!(links[1].opens.is_empty());
        assert_eq!(links[2].label, "Play");
    }

    #[test]
    fn improve_offline_keeps_the_prompt() {
        let p = improve_offline("a spooky hotel");
        assert!(p.starts_with("a spooky hotel. HUD: "));
        assert!(p.contains("Screens: "));
    }
}
