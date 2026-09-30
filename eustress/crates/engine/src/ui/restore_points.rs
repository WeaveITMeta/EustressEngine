//! The History panel's Restore points view: the Space's snapshots and revert
//! records, Save, and a Revert behind the panel's confirm.
//!
//! Everything it does runs through [`crate::space::snapshot`], the core the
//! MCP snapshot tools use, so the panel and an agent keep the same rules and
//! are refused for the same reasons in the same words. What the view shows is
//! read from the Space's history on a worker thread, so a long history never
//! stalls a frame; it refreshes when the view opens, when the open Space
//! changes, and after every snapshot, revert or cancel, whoever asked.
//!
//! The panel's Slint callbacks reach this module as commands queued by
//! `drain_slint_actions`, which gives each one `&mut World` without adding a
//! parameter to the drain.

use bevy::prelude::*;
use std::path::PathBuf;
use std::sync::{mpsc, Mutex};

use crate::space::snapshot::{self, Origin, PointKind, RestorePoint};

/// Rows the view reads from the history.
const LIMIT: usize = 200;
/// How often the reasons Save and Revert are refused are read again.
const BLOCKER_EVERY_SECS: f64 = 0.5;

pub struct RestorePointsPlugin;

impl Plugin for RestorePointsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RestorePointsView>()
            .add_systems(Update, sync_restore_points);
    }
}

type Loaded = Result<Vec<RestorePoint>, String>;

#[derive(Resource, Default)]
pub(crate) struct RestorePointsView {
    rows: Vec<RestorePoint>,
    status: String,
    status_error: bool,
    blocker: String,
    pending: bool,
    busy: String,
    /// Bumped on every change the Slint side must show.
    revision: u64,
    shown_revision: u64,
    /// The `SnapshotsChanged` count and Space the rows were read for.
    seen_changes: u64,
    seen_root: Option<PathBuf>,
    refresh_wanted: bool,
    /// A history read running on a worker thread.
    worker: Option<Mutex<mpsc::Receiver<Loaded>>>,
    next_blocker_check: f64,
}

impl RestorePointsView {
    fn set_status(&mut self, text: impl Into<String>, error: bool) {
        self.status = text.into();
        self.status_error = error;
        self.revision += 1;
    }
}

// ---------------------------------------------------------------------------
// What the panel's callbacks run
// ---------------------------------------------------------------------------

/// The Restore points view opened: read the history again.
pub(crate) fn request_refresh(world: &mut World) {
    if let Some(mut view) = world.get_resource_mut::<RestorePointsView>() {
        view.refresh_wanted = true;
    }
}

/// Save a restore point. An empty label is "Restore point".
pub(crate) fn save(world: &mut World, label: String) {
    let label = snapshot::clean_label(&label);
    let label = if label.is_empty() { "Restore point".to_string() } else { label };
    let result = snapshot::take_snapshot(world, &label, &Origin::Studio, None, snapshot::Prune::Now);
    let Some(mut view) = world.get_resource_mut::<RestorePointsView>() else { return };
    match result {
        Ok(_) => view.set_status(format!("Saved '{label}'."), false),
        Err(e) => view.set_status(format!("Not saved: {e}."), true),
    }
    view.refresh_wanted = true;
}

/// Revert to snapshot `id`. The panel calls this only after its confirm.
pub(crate) fn revert(world: &mut World, id: String) {
    let reply: snapshot::RevertReply = Box::new(|world: &mut World, outcome: &snapshot::RevertOutcome| {
        let Some(mut view) = world.get_resource_mut::<RestorePointsView>() else { return };
        match &outcome.result {
            Ok(_) => view.set_status(
                format!(
                    "Restored '{}'. To undo, revert to the Safety point saved just before it.",
                    outcome.target_label
                ),
                false,
            ),
            Err(e) => view.set_status(format!("{}.", capitalise(e)), true),
        }
        view.refresh_wanted = true;
    });
    if let Err(e) = snapshot::start_revert(world, &id, None, Origin::Studio, reply) {
        if let Some(mut view) = world.get_resource_mut::<RestorePointsView>() {
            view.set_status(format!("Not reverted: {e}."), true);
        }
    }
}

/// Withdraw a revert waiting for the Space to reopen.
pub(crate) fn cancel_pending(world: &mut World) {
    let result = snapshot::cancel_pending(world);
    let Some(mut view) = world.get_resource_mut::<RestorePointsView>() else { return };
    match result {
        Ok(true) => view.set_status("Cancelled the waiting revert. The Space stays as it is.", false),
        Ok(false) => view.set_status("No revert was waiting.", false),
        Err(e) => view.set_status(format!("{}.", capitalise(&e)), true),
    }
    view.next_blocker_check = 0.0;
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Keeping the view current
// ---------------------------------------------------------------------------

fn sync_restore_points(world: &mut World) {
    if world.get_resource::<RestorePointsView>().is_none() {
        return;
    }
    let root = world.get_resource::<crate::space::SpaceRoot>().map(|r| r.0.clone());
    let changes = world
        .get_resource::<snapshot::SnapshotsChanged>()
        .map_or(0, |c| c.0);
    let now = world
        .get_resource::<Time>()
        .map_or(0.0, |t| t.elapsed_secs_f64());

    // The reasons Save and Revert are refused, read on a timer: one of them
    // is a file on disk.
    let due = world.resource::<RestorePointsView>().next_blocker_check <= now;
    let blockers = if due { Some(snapshot::blockers(world)) } else { None };

    let mut view = world.resource_mut::<RestorePointsView>();
    if view.seen_changes != changes || view.seen_root != root {
        view.seen_changes = changes;
        if view.seen_root != root {
            view.seen_root = root.clone();
            view.rows.clear();
            view.status.clear();
            view.revision += 1;
        }
        view.refresh_wanted = true;
    }

    // Collect a finished read.
    let finished = view
        .worker
        .as_ref()
        .and_then(|w| w.lock().ok().and_then(|rx| rx.try_recv().ok()));
    if let Some(loaded) = finished {
        view.worker = None;
        match loaded {
            Ok(rows) => view.rows = rows,
            Err(e) => view.set_status(format!("Could not read the restore points: {e}."), true),
        }
        view.revision += 1;
    }

    // Start one when wanted and none is running.
    if view.refresh_wanted && view.worker.is_none() {
        view.refresh_wanted = false;
        if let Some(root) = root {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(snapshot::history(&root, LIMIT));
            });
            view.worker = Some(Mutex::new(rx));
        }
    }

    if let Some(b) = blockers {
        view.next_blocker_check = now + BLOCKER_EVERY_SECS;
        let blocker = b.save.or(b.revert).map(|s| capitalise(&s)).unwrap_or_default();
        let busy = b.busy.map(|label| format!("Reverting to '{label}'")).unwrap_or_default();
        if blocker != view.blocker || busy != view.busy || b.pending != view.pending {
            view.blocker = blocker;
            view.busy = busy;
            view.pending = b.pending;
            view.revision += 1;
        }
    }

    let up_to_date = view.revision == view.shown_revision;
    drop(view);
    if up_to_date {
        return;
    }
    let Some(ui) = world
        .get_non_send_resource::<crate::ui::slint_ui::SlintUiState>()
        .map(|state| &state.window)
    else {
        return;
    };
    let view = world.resource::<RestorePointsView>();
    let today = chrono::Local::now().date_naive();
    let rows: Vec<crate::ui::slint_ui::RestorePointRow> = view.rows.iter().map(|p| row(p, today)).collect();
    ui.set_restore_points(slint::ModelRc::new(slint::VecModel::from(rows)));
    ui.set_restore_points_blocker(view.blocker.clone().into());
    ui.set_restore_points_pending(view.pending);
    ui.set_restore_points_busy(view.busy.clone().into());
    ui.set_restore_points_status(view.status.clone().into());
    ui.set_restore_points_status_error(view.status_error);
    let shown = view.revision;
    world.resource_mut::<RestorePointsView>().shown_revision = shown;
}

/// One row as the panel shows it.
fn row(p: &RestorePoint, today: chrono::NaiveDate) -> crate::ui::slint_ui::RestorePointRow {
    let (time, time_full) = match chrono::DateTime::parse_from_rfc3339(&p.created) {
        Ok(t) => {
            let local = t.with_timezone(&chrono::Local);
            let time = if local.date_naive() == today {
                local.format("%H:%M").to_string()
            } else {
                local.format("%b %-d %H:%M").to_string()
            };
            (time, local.format("%a %b %-d, %H:%M:%S").to_string())
        }
        Err(_) => (p.created.clone(), p.created.clone()),
    };
    let who = if p.by_studio {
        "you".to_string()
    } else {
        match &p.agent {
            Some(name) => format!("an agent ({name})"),
            None => "an agent".to_string(),
        }
    };
    let (origin, note) = match &p.kind {
        PointKind::Safety => ("safety", p.note.clone()),
        PointKind::Revert { failed: true } => ("revert", format!("By {who}; withdrawn, nothing changed")),
        PointKind::Revert { failed: false } => ("revert", format!("By {who}. {}", capitalise(&p.note))),
        PointKind::Snapshot => (if p.by_studio { "you" } else { "agent" }, p.note.clone()),
    };
    crate::ui::slint_ui::RestorePointRow {
        id: p.id.clone().into(),
        label: p.label.clone().into(),
        time: time.into(),
        time_full: time_full.into(),
        author: p.author.clone().into(),
        origin: origin.into(),
        revertible: p.revertible,
        note: capitalise(&note).into(),
    }
}
