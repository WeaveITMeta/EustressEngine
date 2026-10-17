//! # Find in Scripts (Ctrl+Shift+F)
//!
//! The Studio side of the modal in `ui/slint/script_search.slint`. The window
//! sends its typing and keys as `SlintAction::ScriptSearch*`; the drain only
//! records them on [`StudioState::script_search`] (a [`SearchState`]). Then
//! [`sync_script_search`], once a frame and in this order:
//!
//! 1. opens the modal when the shortcut bumped `open_script_search_pulse`;
//! 2. searches again when the query, the case switch or the open modal asks;
//! 3. opens the chosen row's script in a tab;
//! 4. selects the matched text once that tab shows its text (the hit counts in
//!    characters; the editor's offsets are bytes, so
//!    [`script_search::selection_for`] converts against the buffer the editor
//!    holds), and scrolls its line into view;
//! 5. pushes the list, the status line and the highlighted row to Slint.
//!
//! It reads every script the Space holds and changes none: it opens tabs and
//! selects a match, never an edit. It does not check the Play state, so it
//! works in a Play session as well. It stands alone: the strings a row draws
//! are bounded by [`script_search::row_text`], not by another module.
//!
//! ## What is searched
//!
//! Each script component in the engine, one per script entity:
//!
//! | Component | Holds | Where its text comes from |
//! |---|---|---|
//! | [`SoulScriptData`] | Rune, Luau, Lua and Soul markdown (`run_context`) | `source` |
//! | [`MluaScript`], [`MluaLocalScript`], [`MluaModuleScript`] | Luau | `source` |
//!
//! A script open in a tab is searched as the tab shows it, unsaved edits
//! included, so a result's line is the line the editor opens at. A Workshop
//! conversation also carries a [`SoulScriptData`] (its transcript) and is left
//! out: it is not a script.

use bevy::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::center_tabs::{CenterTabEntry, CenterTabManager, CenterTabType, SoulScriptMode};
use super::slint_ui::{ScriptSearchRow, SlintUiState, StudioWindow};
use super::StudioState;
use crate::script_editor::script_search::{
    self, keep_blanks, location_label, row_text, selection_for, split_for_display, Jump, JumpStep, ScriptHit, ScriptText, SearchOutcome, MAX_LABEL_CHARS, MAX_ROW_TEXT_CHARS,
};
use crate::soul::SoulScriptData;
use crate::space::LoadedFromFile;
use eustress_common::classes::{ClassName, Instance};
use eustress_common::mlua::{MluaLocalScript, MluaModuleScript, MluaScript};

/// A script the modal can search, with what opening it takes.
struct Gathered {
    entity: Entity,
    name: String,
    /// What a row says: the path inside the Space, or "Space database".
    location: String,
    source: String,
    /// What `CenterTabManager::open_file` opens, when the script has a file.
    file: Option<PathBuf>,
}

/// The queries over every kind of script component.
#[derive(bevy::ecs::system::SystemParam)]
pub struct ScriptSources<'w, 's> {
    soul: Query<'w, 's, (Entity, &'static Instance, &'static SoulScriptData, Option<&'static LoadedFromFile>)>,
    mlua: Query<'w, 's, (Entity, &'static Instance, &'static MluaScript, Option<&'static LoadedFromFile>)>,
    local: Query<'w, 's, (Entity, &'static Instance, &'static MluaLocalScript, Option<&'static LoadedFromFile>)>,
    module: Query<'w, 's, (Entity, &'static Instance, &'static MluaModuleScript, Option<&'static LoadedFromFile>)>,
}

impl ScriptSources<'_, '_> {
    /// Every script in the Space, as owned text, sorted by location and name.
    /// `only` keeps the one script whose entity has these bits.
    fn gather(&self, tabs: Option<&CenterTabManager>, space_root: Option<&Path>, only: Option<u64>) -> Vec<Gathered> {
        let mut scripts: Vec<Gathered> = Vec::new();
        let mut seen: HashSet<Entity> = HashSet::new();
        let mut take = |entity: Entity, instance: &Instance, source: &str, own_file: Option<&str>, loaded: Option<&LoadedFromFile>| {
            if only.is_some_and(|id| id != entity.to_bits()) || !seen.insert(entity) {
                return;
            }
            let file = match own_file.filter(|path| !path.is_empty()) {
                Some(path) => Some(PathBuf::from(path)),
                None => loaded.map(|loaded| script_file(&loaded.path)),
            };
            let text = open_tab_text(tabs, entity, file.as_deref()).unwrap_or(source);
            scripts.push(Gathered {
                entity,
                name: instance.name.clone(),
                location: location_label(file.as_deref(), space_root),
                source: text.to_string(),
                file,
            });
        };
        for (entity, instance, data, loaded) in self.soul.iter() {
            if instance.class_name != ClassName::WorkshopConversation {
                take(entity, instance, &data.source, None, loaded);
            }
        }
        for (entity, instance, script, loaded) in self.mlua.iter() {
            take(entity, instance, &script.source, Some(&script.source_path), loaded);
        }
        for (entity, instance, script, loaded) in self.local.iter() {
            take(entity, instance, &script.source, Some(&script.source_path), loaded);
        }
        for (entity, instance, script, loaded) in self.module.iter() {
            take(entity, instance, &script.source, Some(&script.source_path), loaded);
        }
        scripts.sort_by_cached_key(|g| (g.location.to_lowercase(), g.name.to_lowercase(), g.entity.to_bits()));
        scripts
    }
}

/// What the tab manager opens a script by: its file, or the folder holding
/// the `_instance.toml` its `LoadedFromFile` names (as the Explorer does).
fn script_file(path: &Path) -> PathBuf {
    if path.file_name().is_some_and(|name| name == "_instance.toml") {
        path.parent().map_or_else(|| path.to_path_buf(), Path::to_path_buf)
    } else {
        path.to_path_buf()
    }
}

/// The text of the open tab that shows this script, if there is one: what the
/// editor shows, unsaved edits included.
fn open_tab_text<'a>(tabs: Option<&'a CenterTabManager>, entity: Entity, file: Option<&Path>) -> Option<&'a str> {
    tabs?
        .tabs
        .iter()
        .find(|tab| {
            matches!(tab.tab_type, CenterTabType::SoulScript { .. } | CenterTabType::CodeEditor { .. })
                && (tab.entity == Some(entity) || file.is_some_and(|file| tab.file_path.as_deref() == Some(file)))
        })
        .map(|tab| tab.content.as_str())
}

/// Open the script a match belongs to in a tab, showing its code, and note
/// which match to select once the tab has its text.
fn open_hit(state: &mut StudioState, tabs: Option<&mut CenterTabManager>, scripts: &ScriptSources, space_root: Option<&Path>, hit: ScriptHit) {
    let Some(tabs) = tabs else {
        return;
    };
    let Some(script) = scripts.gather(Some(&*tabs), space_root, Some(hit.id)).into_iter().next() else {
        // The script went away since the search; search again so the list is true.
        warn!("Find in Scripts: {} is no longer in the Space", hit.name);
        state.script_search.stale = true;
        return;
    };
    let index = match &script.file {
        Some(file) => tabs.open_file(file),
        None => tabs.open_soul_script(script.entity, &script.name, &script.source),
    };
    // A script whose file is not on disk (the Space database holds the text)
    // opens an empty tab: give it the text the search read.
    if let Some(tab) = tabs.tabs.get_mut(index) {
        if tab.content.is_empty() && !tab.dirty && !script.source.is_empty() {
            tab.content = script.source.clone();
        }
    }
    // The search read the code, so show the code, not the script's summary.
    if matches!(tabs.tabs.get(index).map(|tab| &tab.tab_type), Some(CenterTabType::SoulScript { mode: SoulScriptMode::Summary })) {
        tabs.toggle_mode(index);
        // The strip shows the mode, so it needs the full rebuild.
        tabs.focus_only = false;
    }
    if let Some(tab) = tabs.tabs.get(index) {
        state.script_search.jump = Some(Jump::new(tab.id, hit.line, hit.col, hit.chars));
    }
    info!("🔎 Find in Scripts: opening {} at line {}", script.name, hit.line);
}

/// Whether `tab` is the one the editor is showing, with its own text in it and
/// no tab switch in flight.
fn tab_shows_its_text(state: &StudioState, tabs: &CenterTabManager, tab: &CenterTabEntry, ui: &StudioWindow) -> bool {
    let editor = ui.get_active_tab_type();
    let showing_code = matches!(editor.as_str(), "script" | "code");
    let synced = !tabs.dirty && !state.tabs_dirty && state.tabs_deferred_frames == 0 && !state.script_content_dirty;
    showing_code && synced && state.script_editor_content == tab.content
}

/// Select the match and scroll its line into the upper third of the editor. The
/// selection travels as an external edit, which the editor applies together
/// with its text; a selection draws without the editor holding the keyboard, so
/// the person sees where the jump landed. The hit counts in characters and the
/// editor takes UTF-8 byte offsets, so the range is made against the buffer the
/// editor holds. Where there is nothing to select it is an empty range at the
/// start of the line.
fn select_match(state: &mut StudioState, ui: &StudioWindow, jump: &Jump) {
    let (from, to) = selection_for(&state.script_editor_content, jump.line, jump.col, jump.chars);
    state.pending_external_edit = Some((from as u32, to as u32));
    let visible = ui.get_script_visible_line_count().max(3);
    ui.set_script_scroll_to_line((jump.line as i32 - visible / 3).max(1));
}

/// One hit as a Slint row. A row draws strings that came from outside (a
/// script's name, its path, a line of its text), so each is bounded by
/// [`row_text`], which keeps a Text drawable at any size.
fn row_of(hit: &ScriptHit) -> ScriptSearchRow {
    let (before, matched, after) = split_for_display(&hit.text, hit.match_start, hit.match_len);
    let part = |s: &str| slint::SharedString::from(keep_blanks(&row_text(s, MAX_ROW_TEXT_CHARS)));
    ScriptSearchRow {
        name: row_text(&hit.name, MAX_LABEL_CHARS).into(),
        location: row_text(&hit.location, MAX_LABEL_CHARS).into(),
        line: hit.line as i32,
        text: row_text(&hit.text, MAX_ROW_TEXT_CHARS).into(),
        match_start: hit.match_start as i32,
        match_len: hit.match_len as i32,
        before: part(before),
        hit: part(matched),
        after: part(after),
    }
}

/// The Find in Scripts system. See the module docs for what it does, in order.
pub fn sync_script_search(
    slint_context: Option<NonSend<SlintUiState>>,
    state: Option<ResMut<StudioState>>,
    tabs: Option<ResMut<CenterTabManager>>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    scripts: ScriptSources,
    mut seen_pulse: Local<u32>,
) {
    let (Some(context), Some(mut state)) = (slint_context, state) else {
        return;
    };
    let mut tabs = tabs;
    let ui = &context.window;
    let root = space_root.as_deref().map(|root| root.0.as_path());

    // 1. Opened by the shortcut (or by the editor, when a script holds the keyboard).
    if state.open_script_search_pulse != *seen_pulse {
        *seen_pulse = state.open_script_search_pulse;
        state.script_search.reopen();
        ui.set_show_script_search(true);
    }

    // 2. The query, the case switch or the scripts changed.
    if state.script_search.stale {
        let query = state.script_search.query.clone();
        let match_case = state.script_search.match_case;
        if query.trim().is_empty() {
            state.script_search.finish(SearchOutcome::default(), 0);
        } else {
            let all = scripts.gather(tabs.as_deref(), root, None);
            let texts: Vec<ScriptText> = all
                .iter()
                .map(|script| ScriptText {
                    id: script.entity.to_bits(),
                    name: &script.name,
                    location: &script.location,
                    source: &script.source,
                })
                .collect();
            let outcome = script_search::search(&texts, &query, match_case);
            state.script_search.finish(outcome, all.len());
        }
    }

    // 3. A click or Enter on a row.
    if state.script_search.open.is_some() {
        if let Some(hit) = state.script_search.open.take() {
            open_hit(&mut state, tabs.as_deref_mut(), &scripts, root, hit);
        }
    }

    // 4. The opened tab is showing its text: select the match.
    if state.script_search.jump.is_some() {
        if let Some(mut jump) = state.script_search.jump.take() {
            let step = match tabs.as_deref() {
                Some(tabs) => {
                    let active = tabs.active();
                    let settled = active.is_some_and(|tab| tab_shows_its_text(&state, tabs, tab, ui));
                    jump.step(active.map(|tab| tab.id) == Some(jump.tab), settled)
                }
                None => JumpStep::Drop,
            };
            match step {
                JumpStep::Wait => state.script_search.jump = Some(jump),
                JumpStep::Apply => select_match(&mut state, ui, &jump),
                JumpStep::Drop => {}
            }
        }
    }

    // 5. What the window is missing. Checked before borrowing the state
    // mutably, so a quiet frame does not mark it changed.
    if state.script_search.list_changed {
        let search = &mut state.script_search;
        search.list_changed = false;
        let rows: Vec<ScriptSearchRow> = search.hits.iter().map(row_of).collect();
        ui.set_script_search_results(slint::ModelRc::new(slint::VecModel::from(rows)));
        ui.set_script_search_status(search.status().into());
    }
    if state.script_search.selection_changed {
        let search = &mut state.script_search;
        search.selection_changed = false;
        ui.set_script_search_selected(search.selected as i32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_instance_marker_opens_as_its_folder() {
        assert_eq!(script_file(Path::new("S/Door/_instance.toml")), PathBuf::from("S/Door"));
        assert_eq!(script_file(Path::new("S/Door/Door.rune")), PathBuf::from("S/Door/Door.rune"));
        assert_eq!(script_file(Path::new("_instance.toml")), PathBuf::from(""));
    }

    #[test]
    fn an_open_tab_is_searched_as_it_shows() {
        let mut tabs = CenterTabManager::default();
        let entity = Entity::PLACEHOLDER;
        assert_eq!(open_tab_text(Some(&tabs), entity, None), None, "no tab yet");
        let index = tabs.open_soul_script(entity, "Door", "saved text");
        assert_eq!(open_tab_text(Some(&tabs), entity, None), Some("saved text"));
        // An unsaved edit is what the editor shows, so it is what is searched.
        tabs.tabs[index].content = "edited text".to_string();
        assert_eq!(open_tab_text(Some(&tabs), entity, None), Some("edited text"));
        assert_eq!(open_tab_text(None, entity, None), None);
        // A tab that is not a script's does not stand in for it.
        let mut web = CenterTabManager::default();
        web.open_web_tab("about:blank", "Web");
        assert_eq!(open_tab_text(Some(&web), entity, None), None);
    }

    #[test]
    fn a_row_cuts_its_line_at_the_match_and_bounds_every_string() {
        let hit = ScriptHit {
            id: 1,
            name: "Door".to_string(),
            location: "SoulService/Door/Door.rune".to_string(),
            line: 12,
            text: "local x = spawn(1)".to_string(),
            match_start: 10,
            match_len: 5,
            col: 10,
            chars: 5,
        };
        let row = row_of(&hit);
        assert_eq!((row.line, row.match_start, row.match_len), (12, 10, 5));
        assert_eq!(row.hit.as_str(), "spawn");
        // Blanks at the edges are no-break spaces, so the parts measure true.
        assert_eq!(row.before.as_str(), "local\u{a0}x\u{a0}=\u{a0}");
        assert_eq!(row.after.as_str(), "(1)");

        let long = "W".repeat(5_000);
        let huge = ScriptHit {
            name: long.clone(),
            location: long.clone(),
            text: long.clone(),
            match_start: 0,
            match_len: 3,
            col: 0,
            chars: 3,
            ..hit
        };
        let row = row_of(&huge);
        for part in [&row.name, &row.location] {
            assert_eq!(part.chars().count(), MAX_LABEL_CHARS);
            assert!(part.ends_with('…'));
        }
        for part in [&row.text, &row.before, &row.hit, &row.after] {
            assert!(part.chars().count() <= MAX_ROW_TEXT_CHARS, "a string went through unbounded: {} characters", part.chars().count());
        }
        // A name with a line break or a tab draws on one line.
        let odd = ScriptHit {
            name: "Door\n2\tB".to_string(),
            ..huge
        };
        assert_eq!(row_of(&odd).name.as_str(), "Door 2 B");
    }
}
