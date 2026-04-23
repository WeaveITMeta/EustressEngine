//! Rust → Slint sync for the Timeline panel.
//!
//! Reflects `TimelineState` + `TimelineFilter` + `BottomPanelMode`
//! into the main.slint properties (`timeline-tracks`,
//! `timeline-markers`, `timeline-kind-chips`, `timeline-filter-rows`,
//! `bottom-panel-mode`). Drains the matching Slint callbacks back
//! into Bevy events.
//!
//! ## Why a separate module
//!
//! `slint_ui.rs` is already 8k+ lines; the Timeline sync doesn't
//! need the DrainEventWriters bundle. Keeping it here lets the
//! Timeline feature iterate in isolation.
//!
//! The sync system runs after `SlintSystems::Drain` so user input
//! from the current frame is reflected before Slint renders.

use bevy::prelude::*;
use crate::timeline_panel::{
    TimelineState, TimelineFilter, BottomPanelMode,
    TimelineEventKind, SetBottomPanelEvent,
};

/// Plugin that wires Rust timeline state to Slint properties. Must
/// be added AFTER the SlintUiPlugin so `SlintUiState` exists.
pub struct TimelineSlintSyncPlugin;

impl Plugin for TimelineSlintSyncPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            sync_bottom_panel_mode,
            sync_timeline_to_slint,
        ));
    }
}

/// Reflect `BottomPanelMode` into the Slint `bottom-panel-mode`
/// string property. Emits `SetBottomPanelEvent` when the Slint
/// callback fires (handled by `timeline_panel::handle_panel_mode`).
fn sync_bottom_panel_mode(
    slint_context: Option<NonSend<crate::ui::SlintUiState>>,
    mode: Option<Res<BottomPanelMode>>,
) {
    let Some(slint_context) = slint_context else { return };
    let Some(mode) = mode else { return };
    let ui = &slint_context.window;

    let target = match *mode {
        BottomPanelMode::Output   => "output",
        BottomPanelMode::Timeline => "timeline",
    };
    let current: String = ui.get_bottom_panel_mode().into();
    if current != target {
        ui.set_bottom_panel_mode(target.into());
    }
}

/// Reflect the timeline event list + filter into Slint properties.
/// Runs every frame when the Timeline mode is active; cheap because
/// `VecModel::from(Vec)` just swaps the backing storage.
fn sync_timeline_to_slint(
    slint_context: Option<NonSend<crate::ui::SlintUiState>>,
    state: Option<Res<TimelineState>>,
    filter: Option<Res<TimelineFilter>>,
    mode: Option<Res<BottomPanelMode>>,
) {
    let Some(slint_context) = slint_context else { return };
    let Some(state) = state else { return };
    let Some(filter) = filter else { return };
    let Some(mode) = mode else { return };
    if *mode != BottomPanelMode::Timeline && !filter.modal_open {
        // Skip the allocation cost when the user can't see the panel.
        return;
    }
    let ui = &slint_context.window;

    // Derive the visible time window — auto-fit if `window` unset.
    let (t_min, t_max) = state.span().unwrap_or((0.0, 1.0));
    let span = (t_max - t_min).max(1.0);

    // Walk events once, filtering + bucketing per tag + kind.
    let mut per_tag_count: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut kind_count: [u32; 3] = [0, 0, 0];
    let mut markers: Vec<TimelineMarkerData> = Vec::new();

    // Track-index assignment — order of first-appearance per tag,
    // stable across frames while the event set is append-only.
    let mut tag_to_index: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
    let mut next_ix: i32 = 0;

    for event in state.events.iter() {
        let tag = event.primary_tag().to_string();
        *per_tag_count.entry(tag.clone()).or_insert(0) += 1;
        kind_count[match event.kind {
            TimelineEventKind::Keyframe   => 0,
            TimelineEventKind::Watchpoint => 1,
            TimelineEventKind::Breakpoint => 2,
        }] += 1;

        if !filter.is_event_visible(event) { continue; }

        let ix = *tag_to_index.entry(tag.clone()).or_insert_with(|| {
            let i = next_ix;
            next_ix += 1;
            i
        });
        let t_norm = ((event.timestamp - t_min) / span).clamp(0.0, 1.0) as f32;
        markers.push(TimelineMarkerData {
            t: t_norm,
            track_index: ix,
            kind: event.kind.as_str().into(),
            label: event.label.as_str().into(),
            tooltip: format!(
                "{:.3}s · {} · [{}]",
                event.timestamp, event.label, event.tags.join(", ")
            ).into(),
        });
    }

    // Build track list in the order tags were first seen.
    let mut tag_order: Vec<(String, i32)> = tag_to_index.into_iter().collect();
    tag_order.sort_by_key(|(_, ix)| *ix);
    let tracks: Vec<TimelineTrackData> = tag_order.iter().map(|(tag, _)| {
        TimelineTrackData {
            tag: tag.as_str().into(),
            visible: filter.tag_visible.get(tag).copied().unwrap_or(true),
            count: *per_tag_count.get(tag).unwrap_or(&0) as i32,
        }
    }).collect();

    let kind_chips: Vec<TimelineFilterChipData> = [
        (TimelineEventKind::Keyframe,   "keyframe",   kind_count[0]),
        (TimelineEventKind::Watchpoint, "watchpoint", kind_count[1]),
        (TimelineEventKind::Breakpoint, "breakpoint", kind_count[2]),
    ].iter().map(|(k, name, count)| {
        TimelineFilterChipData {
            kind: (*name).into(),
            visible: filter.kind_visible.get(k).copied().unwrap_or(true),
            count: *count as i32,
        }
    }).collect();

    // Filter modal rows — one per distinct tag, sorted alphabetically
    // for stable presentation regardless of first-seen order.
    let mut sorted_tags: Vec<(String, u32)> = per_tag_count.into_iter().collect();
    sorted_tags.sort_by(|a, b| a.0.cmp(&b.0));
    let filter_rows: Vec<TimelineTagRow> = sorted_tags.iter().map(|(tag, count)| {
        TimelineTagRow {
            tag: tag.as_str().into(),
            visible: filter.tag_visible.get(tag).copied().unwrap_or(true),
            count: *count as i32,
        }
    }).collect();

    ui.set_timeline_tracks(slint::ModelRc::new(slint::VecModel::from(tracks)));
    ui.set_timeline_markers(slint::ModelRc::new(slint::VecModel::from(markers)));
    ui.set_timeline_kind_chips(slint::ModelRc::new(slint::VecModel::from(kind_chips)));
    ui.set_timeline_filter_rows(slint::ModelRc::new(slint::VecModel::from(filter_rows)));
    ui.set_timeline_search_text(filter.search.as_str().into());
    ui.set_timeline_filter_modal_open(filter.modal_open);
    ui.set_timeline_time_range_label(
        format!("{:.2}s – {:.2}s  ({} events)", t_min, t_max, state.events.len()).as_str().into()
    );

    let _ = SetBottomPanelEvent; // keep import live
}

// Shadow the Slint-generated struct names at this module scope so
// `sync_timeline_to_slint` reads cleanly. Slint-generated types live
// inside the `slint::include_modules!()` macro invocation; they're
// re-exported through the UI root module.
use crate::ui::slint_ui::TimelineTrackData;
use crate::ui::slint_ui::TimelineMarkerData;
use crate::ui::slint_ui::TimelineFilterChipData;
use crate::ui::slint_ui::TimelineTagRow;
