//! # Toast Undo (Phase 0 UX polish)
//!
//! Surfaces a top-centered toast with an inline Undo action whenever
//! a labeled commit lands on the `UndoStack`. The toast reuses the
//! label set by `UndoStack::push_labeled(...)` so users see a
//! meaningful confirmation ("Mirrored 324 parts") instead of a generic
//! one. Auto-dismisses after 5 seconds; hovering freezes the timer.
//!
//! Builds on existing `NotificationManager` machinery — the toast
//! itself is a dedicated Slint component (`toast_undo.slint`) rather
//! than a notification row because it needs an inline button.

use bevy::prelude::*;

// ============================================================================
// State
// ============================================================================

#[derive(Resource, Debug, Clone)]
pub struct ToastUndoState {
    pub visible: bool,
    pub message: String,
    pub undo_shortcut: String,
    /// Seconds remaining in the show cycle. Decremented by a per-frame
    /// system; when it hits 0 the toast dismisses itself.
    pub remaining: f32,
    pub duration: f32,
    /// 0..1 fade progress for the Slint component.
    pub progress: f32,
    /// True when cursor is over the toast — Slint reflects this back
    /// and Rust pauses the timer.
    pub hovered: bool,
}

impl Default for ToastUndoState {
    fn default() -> Self {
        Self {
            visible: false,
            message: String::new(),
            undo_shortcut: "Ctrl+Z".to_string(),
            remaining: 0.0,
            duration: 5.0,
            progress: 0.0,
            hovered: false,
        }
    }
}

// ============================================================================
// Events
// ============================================================================

/// Emitted from Slint's `undo-clicked` callback; handler dispatches
/// the same `UndoEvent` that Ctrl+Z would and dismisses the toast.
#[derive(Event, Message, Debug, Default, Clone, Copy)]
pub struct ToastUndoClickedEvent;

/// Emitted from Slint's `dismissed` callback. Just hides.
#[derive(Event, Message, Debug, Default, Clone, Copy)]
pub struct ToastUndoDismissedEvent;

// ============================================================================
// Plugin
// ============================================================================

pub struct ToastUndoPlugin;

impl Plugin for ToastUndoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ToastUndoState>()
            .add_message::<ToastUndoClickedEvent>()
            .add_message::<ToastUndoDismissedEvent>()
            .add_systems(Update, (
                surface_toast_on_labeled_commit,
                drive_toast_timer,
                handle_toast_undo_clicked,
                handle_toast_dismissed,
            ));
    }
}

// ============================================================================
// Systems
// ============================================================================

/// When a `ModalToolCommittedEvent` arrives AND the undo stack has a
/// fresh labeled entry at the top, show the toast with that label.
/// Falls back silently when no label is present (the existing
/// `NotificationManager` toast from `announce_modal_tool_commits`
/// still fires, so the user isn't left in the dark).
fn surface_toast_on_labeled_commit(
    mut events: MessageReader<crate::modal_tool::ModalToolCommittedEvent>,
    undo_stack: Res<crate::undo::UndoStack>,
    mut state: ResMut<ToastUndoState>,
) {
    for _ in events.read() {
        let Some(label) = undo_stack.last_label() else { continue };
        state.visible = true;
        state.message = label.to_string();
        state.undo_shortcut = "Ctrl+Z".to_string();
        state.remaining = state.duration;
        state.progress = 1.0;
    }
}

fn drive_toast_timer(time: Res<Time>, mut state: ResMut<ToastUndoState>) {
    if !state.visible { return; }
    if state.hovered { return; } // pause while user's pointing at it
    state.remaining -= time.delta_secs();
    // Fade out during the last 180ms.
    let fade_window = 0.18;
    if state.remaining <= fade_window {
        state.progress = (state.remaining / fade_window).clamp(0.0, 1.0);
    } else {
        state.progress = 1.0;
    }
    if state.remaining <= 0.0 {
        state.visible = false;
        state.progress = 0.0;
    }
}

fn handle_toast_undo_clicked(
    mut events: MessageReader<ToastUndoClickedEvent>,
    mut undo_writer: MessageWriter<crate::undo::UndoEvent>,
    mut state: ResMut<ToastUndoState>,
) {
    for _ in events.read() {
        undo_writer.write(crate::undo::UndoEvent);
        state.visible = false;
        state.progress = 0.0;
    }
}

fn handle_toast_dismissed(
    mut events: MessageReader<ToastUndoDismissedEvent>,
    mut state: ResMut<ToastUndoState>,
) {
    for _ in events.read() {
        state.visible = false;
        state.progress = 0.0;
    }
}
