//! History → Eustress Stream bridge.
//!
//! Every push onto `UndoStack` queues a [`PendingHistoryStreamEvent`]; this
//! plugin drains that queue each frame and tees the entries onto the
//! in-process `EustressStream` on the `history.<kind>` topic.
//!
//! Subscribers (MCP server, LSP, CLI replay, remote debuggers) see every
//! engine mutation in sequential order without touching `UndoStack` or
//! importing `bevy`. The wire format is one JSON line per event:
//!
//! ```json
//! {"seq":42,"kind":"move","topic":"history.move","description":"Move Part","label":null}
//! ```
//!
//! The stream retains its own ring buffer, so late subscribers can replay
//! recent history without the engine re-publishing anything.
use bevy::prelude::*;

use crate::undo::UndoStack;

/// Bevy plugin that teeing `UndoStack` pushes onto the in-process
/// `EustressStream`. Add after `UndoPlugin` + any plugin that installs
/// the `ChangeQueue` resource (engine main does this in its startup
/// sequence; see `plugins()` in `main.rs`).
pub struct HistoryStreamPlugin;

impl Plugin for HistoryStreamPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, publish_history_stream);
    }
}

fn publish_history_stream(
    undo_stack: Option<ResMut<UndoStack>>,
    queue: Option<Res<eustress_common::change_queue::ChangeQueue>>,
) {
    let (Some(mut undo_stack), Some(queue)) = (undo_stack, queue) else { return };

    // Drained without change detection: the pending stream is not history
    // the panel shows, and a mutable drain every frame marked the whole stack
    // changed every frame, so the History panel rebuilt its rows each frame
    // (dropping clicks on them) and could not tell a real edit from none.
    let events = undo_stack.bypass_change_detection().drain_pending_stream();
    if events.is_empty() { return; }

    for ev in events {
        let payload = serde_json::json!({
            "seq": ev.sequence,
            "kind": ev.kind,
            "topic": ev.topic,
            "description": ev.description,
            "label": ev.label,
        });
        let bytes = match serde_json::to_vec(&payload) {
            Ok(b) => b,
            Err(_) => continue,
        };
        queue.stream.producer(&ev.topic).send_bytes(bytes::Bytes::from(bytes));
    }
}
