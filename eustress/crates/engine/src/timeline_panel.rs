//! # Timeline Panel (Phase 2)
//!
//! Data-agnostic timeline that subscribes to Eustress Streams and
//! renders three marker kinds along a horizontal time axis:
//!
//! | Kind        | Glyph     | Color                         | Purpose |
//! |-------------|-----------|-------------------------------|---------|
//! | Keyframe    | diamond   | `accent-yellow`  (`#f9c74f`)  | scheduled / authored snapshot of interest |
//! | Watchpoint  | dot       | `accent-orange`  (`#e8912d`)  | value crossed a threshold / observer tripped |
//! | Breakpoint  | asterisk  | `accent-red`     (`#e74856`)  | execution paused / user attention required |
//!
//! ## Stream-driven
//!
//! Every event in the timeline is a `TimelineEvent` emitted onto the
//! Stream topic `"timeline.*"`. Anything in the engine that wants to
//! surface a marker publishes to that topic; the panel subscribes
//! and renders. No engine subsystem hard-codes knowledge of the
//! panel — they emit stream events, the panel collects.
//!
//! ## Tags as tracks
//!
//! Each `TimelineEvent` carries `tags: Vec<String>`. The panel groups
//! events into horizontal tracks by first-tag; the user's checkbox
//! filter (`TimelineFilter`) toggles which tracks are visible.
//!
//! ## Panel-space sharing with Output
//!
//! The timeline shares the bottom panel space with Output. A new
//! `BottomPanelMode` resource carries which panel is active
//! (`Output` / `Timeline`); title-bar buttons flip the mode.
//!
//! ## Data shape for Slint
//!
//! `TimelineState` reflects a cap-bounded ring of the most recent
//! events into `TimelineStateReflect` each frame — that's what the
//! Slint side reads. Keeps the Slint model allocation stable.

use bevy::prelude::*;
use std::collections::{HashMap, VecDeque};

// ============================================================================
// Data model
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimelineEventKind {
    /// Yellow diamond — scheduled / authored snapshot.
    Keyframe,
    /// Orange dot — value crossed a threshold / observer tripped.
    Watchpoint,
    /// Red asterisk — execution paused / user attention required.
    Breakpoint,
}

impl TimelineEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TimelineEventKind::Keyframe   => "keyframe",
            TimelineEventKind::Watchpoint => "watchpoint",
            TimelineEventKind::Breakpoint => "breakpoint",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "keyframe"   => Some(TimelineEventKind::Keyframe),
            "watchpoint" => Some(TimelineEventKind::Watchpoint),
            "breakpoint" => Some(TimelineEventKind::Breakpoint),
            _ => None,
        }
    }

    /// Default glyph color per the TOOLSET_UX palette.
    pub fn color_hex(self) -> &'static str {
        match self {
            TimelineEventKind::Keyframe   => "#f9c74f", // accent-yellow
            TimelineEventKind::Watchpoint => "#e8912d", // accent-orange
            TimelineEventKind::Breakpoint => "#e74856", // accent-red
        }
    }
}

/// One event on the timeline. Built by any emitter (engine system,
/// tool commit, script assertion, physics sim observer, etc.) and
/// pushed onto the Stream topic `"timeline.*"`.
#[derive(Debug, Clone)]
pub struct TimelineEvent {
    /// Seconds since the engine started (or scenario start when
    /// scenarios carry their own clock). Matches `Time::elapsed_secs`.
    pub timestamp: f64,
    pub kind: TimelineEventKind,
    /// Human-readable single-line label. Rendered next to the marker
    /// on hover.
    pub label: String,
    /// Optional source entity — when set, clicking the marker selects
    /// it in the viewport.
    pub source_entity: Option<Entity>,
    /// Tags group the event into a track. First tag drives the track.
    pub tags: Vec<String>,
    /// Free-form JSON payload for tooltip expansion.
    pub payload: Option<serde_json::Value>,
}

impl TimelineEvent {
    pub fn primary_tag(&self) -> &str {
        self.tags.first().map(|s| s.as_str()).unwrap_or("untagged")
    }
}

// ============================================================================
// State resources
// ============================================================================

/// Cap-bounded ring of all events the panel knows about. Systems
/// push; Slint sync reads.
#[derive(Resource, Debug, Clone)]
pub struct TimelineState {
    pub events: VecDeque<TimelineEvent>,
    pub max_events: usize,
    /// Currently-visible time window, in seconds. `None` = auto-fit
    /// to the full event span.
    pub window: Option<(f64, f64)>,
    /// When true, new events continuously scroll the window to follow
    /// the latest timestamp. Pauses when the user pans manually.
    pub auto_follow: bool,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            events: VecDeque::with_capacity(512),
            max_events: 2048,
            window: None,
            auto_follow: true,
        }
    }
}

impl TimelineState {
    pub fn push(&mut self, event: TimelineEvent) {
        if self.events.len() >= self.max_events {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    pub fn distinct_tags(&self) -> Vec<String> {
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for e in &self.events {
            if let Some(t) = e.tags.first() { set.insert(t.clone()); }
        }
        set.into_iter().collect()
    }

    pub fn span(&self) -> Option<(f64, f64)> {
        if self.events.is_empty() { return None; }
        let first = self.events.front().unwrap().timestamp;
        let last  = self.events.back().unwrap().timestamp;
        Some((first, last))
    }
}

/// User filter state — per-tag + per-kind toggle flags.
#[derive(Resource, Debug, Clone, Default)]
pub struct TimelineFilter {
    /// `tag -> visible?`. Missing entry = default visible = true.
    pub tag_visible: HashMap<String, bool>,
    /// `kind -> visible?`.
    pub kind_visible: HashMap<TimelineEventKind, bool>,
    /// Free-text filter — matches label + tags. Empty = show all.
    pub search: String,
    /// Whether the filter-modal is currently open.
    pub modal_open: bool,
}

impl TimelineFilter {
    pub fn is_event_visible(&self, event: &TimelineEvent) -> bool {
        if !self.kind_visible.get(&event.kind).copied().unwrap_or(true) {
            return false;
        }
        let tag = event.primary_tag();
        if !self.tag_visible.get(tag).copied().unwrap_or(true) {
            return false;
        }
        if !self.search.is_empty() {
            let needle = self.search.to_ascii_lowercase();
            let hay = format!("{} {}", event.label, event.tags.join(" ")).to_ascii_lowercase();
            if !hay.contains(&needle) { return false; }
        }
        true
    }
}

/// Which bottom panel is currently visible. Output is the default
/// (pre-Timeline behavior); Timeline flips in when the user clicks
/// the title button.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomPanelMode {
    Output,
    Timeline,
}

impl Default for BottomPanelMode {
    fn default() -> Self { BottomPanelMode::Output }
}

// ============================================================================
// Events (for emitters)
// ============================================================================

/// Fire this event from anywhere in the engine to push a marker onto
/// the timeline. Preferred over directly touching `TimelineState`
/// because the handler also tees the event onto the Stream for
/// external subscribers (MCP, recording, etc.).
#[derive(Event, Message, Debug, Clone)]
pub struct PublishTimelineEventEvent(pub TimelineEvent);

/// Fire to flip the bottom panel between Output and Timeline.
#[derive(Event, Message, Debug, Clone, Copy)]
pub struct SetBottomPanelEvent(pub BottomPanelMode);

// ============================================================================
// Plugin
// ============================================================================

pub struct TimelinePanelPlugin;

impl Plugin for TimelinePanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TimelineState>()
            .init_resource::<TimelineFilter>()
            .init_resource::<BottomPanelMode>()
            .add_message::<PublishTimelineEventEvent>()
            .add_message::<SetBottomPanelEvent>()
            .add_systems(Update, (
                ingest_published_events,
                handle_panel_mode,
                seed_builtin_emitters,
            ));
    }
}

// ============================================================================
// Systems
// ============================================================================

fn ingest_published_events(
    mut events: MessageReader<PublishTimelineEventEvent>,
    mut state: ResMut<TimelineState>,
    mut filter: ResMut<TimelineFilter>,
) {
    for event in events.read() {
        // Ensure new tags default to visible.
        let tag = event.0.primary_tag().to_string();
        filter.tag_visible.entry(tag).or_insert(true);
        filter.kind_visible.entry(event.0.kind).or_insert(true);
        state.push(event.0.clone());
    }
}

fn handle_panel_mode(
    mut events: MessageReader<SetBottomPanelEvent>,
    mut mode: ResMut<BottomPanelMode>,
) {
    for event in events.read() {
        *mode = event.0;
    }
}

/// Built-in emitters that tee existing engine events onto the
/// timeline. Keeps engine subsystems from needing to know about the
/// panel — they already fire their own events; this adapter converts.
fn seed_builtin_emitters(
    mut commits: MessageReader<crate::modal_tool::ModalToolCommittedEvent>,
    time: Res<Time>,
    mut publish: MessageWriter<PublishTimelineEventEvent>,
) {
    // Every ModalTool commit → Keyframe on the timeline. Other teeing
    // adapters (align/distribute commits, constraint violations,
    // physics assertion trips) can land as additional systems with
    // the same shape.
    for event in commits.read() {
        publish.write(PublishTimelineEventEvent(TimelineEvent {
            timestamp: time.elapsed_secs() as f64,
            kind: TimelineEventKind::Keyframe,
            label: format!("{} committed", event.tool_name),
            source_entity: None,
            tags: vec!["tool.commit".to_string(), event.tool_id.clone()],
            payload: Some(serde_json::json!({
                "tool_id":   event.tool_id,
                "tool_name": event.tool_name,
            })),
        }));
    }
}

// ============================================================================
// Convenience emitters
// ============================================================================

/// Shorthand for emitting a keyframe from any system with a
/// `MessageWriter<PublishTimelineEventEvent>`.
pub fn emit_keyframe(
    w: &mut MessageWriter<PublishTimelineEventEvent>,
    time: f64,
    label: impl Into<String>,
    tags: &[&str],
) {
    w.write(PublishTimelineEventEvent(TimelineEvent {
        timestamp: time,
        kind: TimelineEventKind::Keyframe,
        label: label.into(),
        source_entity: None,
        tags: tags.iter().map(|s| s.to_string()).collect(),
        payload: None,
    }));
}

pub fn emit_watchpoint(
    w: &mut MessageWriter<PublishTimelineEventEvent>,
    time: f64,
    label: impl Into<String>,
    tags: &[&str],
    source_entity: Option<Entity>,
) {
    w.write(PublishTimelineEventEvent(TimelineEvent {
        timestamp: time,
        kind: TimelineEventKind::Watchpoint,
        label: label.into(),
        source_entity,
        tags: tags.iter().map(|s| s.to_string()).collect(),
        payload: None,
    }));
}

pub fn emit_breakpoint(
    w: &mut MessageWriter<PublishTimelineEventEvent>,
    time: f64,
    label: impl Into<String>,
    tags: &[&str],
    source_entity: Option<Entity>,
) {
    w.write(PublishTimelineEventEvent(TimelineEvent {
        timestamp: time,
        kind: TimelineEventKind::Breakpoint,
        label: label.into(),
        source_entity,
        tags: tags.iter().map(|s| s.to_string()).collect(),
        payload: None,
    }));
}
