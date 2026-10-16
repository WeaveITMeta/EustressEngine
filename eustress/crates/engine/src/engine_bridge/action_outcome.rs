//! `action.invoke` answers with what the action did, not only that it was
//! sent. An action runs on a later frame (the editor's own menu path, then
//! the UI's action queue), and a refusal or a dialog shows only in the UI:
//! a PublishSpace refused for a Universe with no listing said so in Output
//! alone, and the caller was told "invoked". So the call is answered a few
//! frames after dispatch, with the Output lines the action wrote, the dialogs
//! it opened, and whether it changed the Space.

use bevy::prelude::*;
use serde_json::{json, Value};

use super::protocol::{BridgeError, BridgeResponse};
use super::server;
use crate::ui::slint_ui::{modal_dialogs, LogLevel, OutputConsole, SlintUiState};

/// Frames between dispatch and the answer: the menu event, the UI action
/// queue, and the frame a dialog or an Output line appears on, with slack.
const SETTLE_FRAMES: u32 = 5;

/// What can be seen before and after an action.
#[derive(Clone, Default)]
struct Seen {
    /// The Output console's revision: every line written after it has a
    /// larger id.
    last_output: u64,
    dialogs: Vec<&'static str>,
    undo_revision: u64,
    playing: bool,
}

/// An action dispatched and waiting for its answer.
struct Job {
    action: String,
    before: Seen,
    frames_left: u32,
    responder: tokio::sync::oneshot::Sender<BridgeResponse>,
    id: Value,
}

/// The actions waiting to be answered.
#[derive(Resource, Default)]
pub(crate) struct ActionJobs(Vec<Job>);

/// The dialogs the Studio window has open: the full-window modals (the one
/// list the input gate keeps, `modal_dialogs`) and Find, which is not one.
/// Empty without a window (a headless engine).
fn open_dialogs(world: &World) -> Vec<&'static str> {
    let Some(ui) = world.get_non_send_resource::<SlintUiState>() else { return Vec::new() };
    let w = &ui.window;
    let mut open: Vec<&'static str> =
        modal_dialogs(w).into_iter().filter(|(_, open)| *open).map(|(name, _)| name).collect();
    if w.get_show_find_dialog() {
        open.push("Find");
    }
    open
}

fn seen(world: &World) -> Seen {
    Seen {
        last_output: world.get_resource::<OutputConsole>().map_or(0, |o| o.revision),
        dialogs: open_dialogs(world),
        undo_revision: world.get_resource::<crate::undo::UndoStack>().map_or(0, |u| u.revision()),
        playing: world
            .get_resource::<State<crate::play_mode::PlayModeState>>()
            .is_some_and(|s| *s.get() != crate::play_mode::PlayModeState::Editing),
    }
}

/// Dispatch `action.invoke` and answer once it has had time to show its
/// effect. An unknown action is refused at once, as before.
pub(crate) fn begin_action_invoke(world: &mut World, pending: server::Pending) {
    // The one parse the permission gate also runs
    // (`eustress_tools::capability::capability_of_call`), so the action run
    // here is the one the gate judged.
    let Some(action) = eustress_common::editor_action::parse_action(&pending.request.params) else {
        let sent = pending.request.params.get("action").and_then(|v| v.as_str()).unwrap_or("");
        let _ = pending.responder.send(BridgeResponse::error(
            pending.request.id.clone(),
            BridgeError::internal(format!(
                "unknown action '{sent}'. Use the Action enum variant name, exactly. The accepted set \
                 is the WHOLE enum, not a safe subset: alongside Copy/Cut/Paste/Duplicate/Group/\
                 Ungroup/SelectAll/Undo/Redo/SaveScene/MoveTool/ScaleTool it includes irreversible \
                 actions (Delete) and outward-facing ones (PublishSpace, PublishUniverse)."
            )),
        ));
        return;
    };
    let before = seen(world);
    world.write_message(crate::ui::MenuActionEvent::new(action));
    let job = Job {
        action: format!("{action:?}"),
        before,
        frames_left: SETTLE_FRAMES,
        responder: pending.responder,
        id: pending.request.id,
    };
    world.get_resource_or_insert_with(ActionJobs::default).0.push(job);
}

/// Answer the actions whose frames have passed.
pub(crate) fn pump_action_jobs(world: &mut World) {
    let Some(mut jobs) = world.get_resource_mut::<ActionJobs>() else { return };
    if jobs.0.is_empty() {
        return;
    }
    let mut due = Vec::new();
    let mut waiting = Vec::new();
    for mut job in std::mem::take(&mut jobs.0) {
        job.frames_left = job.frames_left.saturating_sub(1);
        if job.frames_left == 0 { due.push(job) } else { waiting.push(job) }
    }
    jobs.0 = waiting;
    if due.is_empty() {
        return;
    }
    let after = seen(world);
    for job in due {
        let output = new_output(world, job.before.last_output);
        let report = outcome(&job.action, &job.before, &after, output);
        if job.responder.send(BridgeResponse::ok(job.id, report)).is_err() {
            debug!("EngineBridge: action.invoke caller disconnected before the answer");
        }
    }
}

/// The Output lines written after `since`, oldest first.
fn new_output(world: &World, since: u64) -> Vec<(LogLevel, String)> {
    world
        .get_resource::<OutputConsole>()
        .map(|o| o.entries.iter().filter(|e| e.id > since).map(|e| (e.level, e.message.clone())).collect())
        .unwrap_or_default()
}

fn level_name(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Info => "info",
        LogLevel::Warn => "warning",
        LogLevel::Error => "error",
        LogLevel::Debug => "debug",
    }
}

/// The answer: what was seen to change, and a one-word outcome.
///
/// - `refused`: the action wrote an error to Output.
/// - `opened`: a dialog opened (the action waits on the person there).
/// - `changed`: the History gained or moved a step, or Play started or stopped.
/// - `warned`: it wrote a warning and nothing else changed.
/// - `no visible effect`: none of these. Some actions change only the view.
fn outcome(action: &str, before: &Seen, after: &Seen, output: Vec<(LogLevel, String)>) -> Value {
    let opened: Vec<&str> = after.dialogs.iter().copied().filter(|d| !before.dialogs.contains(d)).collect();
    let undo_step = after.undo_revision != before.undo_revision;
    let play_changed = after.playing != before.playing;
    let has = |level: LogLevel| output.iter().any(|(l, _)| *l == level);
    let outcome = if has(LogLevel::Error) {
        "refused"
    } else if !opened.is_empty() {
        "opened"
    } else if undo_step || play_changed {
        "changed"
    } else if has(LogLevel::Warn) {
        "warned"
    } else {
        "no visible effect"
    };
    json!({
        "invoked": action,
        "outcome": outcome,
        "dialogs_opened": opened,
        "undo_step": undo_step,
        "play_changed": play_changed,
        "output": output
            .iter()
            .map(|(level, message)| json!({ "level": level_name(*level), "message": message }))
            .collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen_with(dialogs: Vec<&'static str>, undo: u64) -> Seen {
        Seen { last_output: 0, dialogs, undo_revision: undo, playing: false }
    }

    /// A refused publish reports its Output line, the reason the caller
    /// could not see before.
    #[test]
    fn a_refusal_is_reported_with_its_reason() {
        let r = outcome(
            "PublishSpace",
            &seen_with(vec![], 1),
            &seen_with(vec![], 1),
            vec![(LogLevel::Error, "Publish the Universe first before publishing individual Spaces.".into())],
        );
        assert_eq!(r["outcome"], "refused");
        assert_eq!(r["output"][0]["level"], "error");
        assert!(r["output"][0]["message"].as_str().unwrap().contains("Publish the Universe first"));
    }

    #[test]
    fn a_dialog_that_opens_is_named() {
        let r = outcome("PublishSpace", &seen_with(vec!["Login"], 1), &seen_with(vec!["Login", "Publish"], 1), vec![]);
        assert_eq!(r["outcome"], "opened");
        assert_eq!(r["dialogs_opened"], json!(["Publish"]));
    }

    #[test]
    fn a_change_and_no_effect_are_told_apart() {
        assert_eq!(outcome("Delete", &seen_with(vec![], 1), &seen_with(vec![], 2), vec![])["outcome"], "changed");
        assert_eq!(outcome("ViewTop", &seen_with(vec![], 1), &seen_with(vec![], 1), vec![])["outcome"], "no visible effect");
        assert_eq!(
            outcome("SaveScene", &seen_with(vec![], 1), &seen_with(vec![], 1), vec![(LogLevel::Warn, "careful".into())])["outcome"],
            "warned"
        );
    }

    /// Only lines written after dispatch belong to the action.
    #[test]
    fn earlier_output_is_not_the_actions() {
        let mut world = World::new();
        let mut console = OutputConsole::default();
        console.info("before");
        let since = console.revision;
        console.error("after");
        world.insert_resource(console);
        let lines = new_output(&world, since);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].1, "after");
    }
}
