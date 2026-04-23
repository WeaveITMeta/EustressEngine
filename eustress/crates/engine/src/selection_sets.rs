//! # Selection Sets (Phase 1)
//!
//! Named, persistent selections. Maya's "quick select sets" — save the
//! current selection under a name, recall it later with one action.
//! Stored per-universe in `.eustress/selection_sets.toml` so the sets
//! survive restart and are git-diffable.
//!
//! ## Schema
//!
//! ```toml
//! [[set]]
//! name = "RoofFrame"
//! part_ids = ["123v4", "125v4", "201v2"]
//! created = "2026-04-21T14:30:00Z"
//! modifier = "alice"
//! ```
//!
//! Part IDs use the same `<entity-index>v<generation>` format as
//! `SelectionManager`, so recall maps cleanly back to live entities.
//!
//! ## Events
//!
//! - `SaveSelectionSetEvent { name }` — writes current `Selected` set
//!   to disk under `name` (overwrites any existing entry).
//! - `LoadSelectionSetEvent { name }` — replaces the live selection
//!   with the set's part_ids.
//! - `DeleteSelectionSetEvent { name }` — removes from disk.
//!
//! Handler systems live in this module; MCP / UI / keybindings fire
//! the events via their usual paths.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::selection_sync::SelectionSyncManager;
use crate::selection_box::Selected;

// ============================================================================
// On-disk representation
// ============================================================================

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SelectionSetsFile {
    #[serde(default, rename = "set")]
    pub sets: Vec<SelectionSet>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionSet {
    pub name: String,
    pub part_ids: Vec<String>,
    pub created: String,
    #[serde(default)]
    pub modifier: String,
}

fn sets_path(space_root: &std::path::Path) -> PathBuf {
    space_root.join(".eustress").join("selection_sets.toml")
}

fn load_sets(space_root: &std::path::Path) -> SelectionSetsFile {
    let path = sets_path(space_root);
    match std::fs::read_to_string(&path) {
        Ok(s) => toml::from_str(&s).unwrap_or_default(),
        Err(_) => SelectionSetsFile::default(),
    }
}

fn save_sets(space_root: &std::path::Path, file: &SelectionSetsFile) -> std::io::Result<()> {
    let path = sets_path(space_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let s = toml::to_string_pretty(file).map_err(|e| std::io::Error::other(e.to_string()))?;
    std::fs::write(path, s)
}

// ============================================================================
// Events
// ============================================================================

#[derive(Event, Message, Debug, Clone)]
pub struct SaveSelectionSetEvent { pub name: String }

#[derive(Event, Message, Debug, Clone)]
pub struct LoadSelectionSetEvent { pub name: String }

#[derive(Event, Message, Debug, Clone)]
pub struct DeleteSelectionSetEvent { pub name: String }

// ============================================================================
// Plugin
// ============================================================================

pub struct SelectionSetsPlugin;

impl Plugin for SelectionSetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SaveSelectionSetEvent>()
            .add_message::<LoadSelectionSetEvent>()
            .add_message::<DeleteSelectionSetEvent>()
            .add_systems(Update, (
                handle_save_selection_set,
                handle_load_selection_set,
                handle_delete_selection_set,
            ));
    }
}

// ============================================================================
// Handlers
// ============================================================================

fn handle_save_selection_set(
    mut events: MessageReader<SaveSelectionSetEvent>,
    selected: Query<Entity, With<Selected>>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    auth: Option<Res<crate::auth::AuthState>>,
) {
    let Some(space) = space_root else { return };
    for event in events.read() {
        let ids: Vec<String> = selected.iter()
            .map(|e| format!("{}v{}", e.index(), e.generation()))
            .collect();
        if ids.is_empty() {
            warn!("⭐ Save Selection Set '{}': no selection", event.name);
            continue;
        }

        let mut file = load_sets(&space.0);
        // Upsert — replace any existing set with the same name.
        file.sets.retain(|s| s.name != event.name);
        let modifier = auth.as_deref()
            .and_then(|a| a.user.as_ref())
            .map(|u| u.username.clone())
            .unwrap_or_default();
        file.sets.push(SelectionSet {
            name: event.name.clone(),
            part_ids: ids.clone(),
            created: chrono::Utc::now().to_rfc3339(),
            modifier,
        });
        match save_sets(&space.0, &file) {
            Ok(_) => info!("⭐ Saved Selection Set '{}' ({} entities)", event.name, ids.len()),
            Err(e) => warn!("⭐ Save Selection Set '{}' failed: {}", event.name, e),
        }
    }
}

fn handle_load_selection_set(
    mut events: MessageReader<LoadSelectionSetEvent>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    selection_manager: Option<Res<SelectionSyncManager>>,
) {
    let Some(space) = space_root else { return };
    let Some(mgr_res) = selection_manager else { return };
    for event in events.read() {
        let file = load_sets(&space.0);
        let Some(set) = file.sets.iter().find(|s| s.name == event.name) else {
            warn!("⭐ Load Selection Set '{}': not found", event.name);
            continue;
        };
        let ids = set.part_ids.clone();
        let mgr = mgr_res.0.write();
        mgr.set_selected(ids);
        info!("⭐ Loaded Selection Set '{}' ({} entities)", event.name, set.part_ids.len());
    }
}

fn handle_delete_selection_set(
    mut events: MessageReader<DeleteSelectionSetEvent>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
) {
    let Some(space) = space_root else { return };
    for event in events.read() {
        let mut file = load_sets(&space.0);
        let before = file.sets.len();
        file.sets.retain(|s| s.name != event.name);
        if file.sets.len() == before {
            warn!("⭐ Delete Selection Set '{}': not found", event.name);
            continue;
        }
        match save_sets(&space.0, &file) {
            Ok(_) => info!("⭐ Deleted Selection Set '{}'", event.name),
            Err(e) => warn!("⭐ Delete Selection Set '{}' failed: {}", event.name, e),
        }
    }
}

/// List all selection sets in the current universe — used by UI /
/// MCP to populate pickers. Synchronous; reads directly from disk.
pub fn list_sets(space_root: &std::path::Path) -> Vec<String> {
    load_sets(space_root).sets.into_iter().map(|s| s.name).collect()
}
