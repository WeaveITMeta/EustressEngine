//! Restore points for the open Space: save one, list them, and go back to
//! one. The engine bridge's `snapshot.*` methods (the MCP snapshot tools) and
//! the History panel both run through this module, so they keep the same
//! rules and refuse for the same reasons in the same words.
//!
//! # What a snapshot is
//!
//! A git commit of the Space's files whose message carries an
//! `Eustress-Snapshot: <id>` trailer. On a database-backed Space it is also a
//! checkpoint of `world.fjalldb` under the same id
//! ([`crate::space::checkpoint`]), because the files are not the whole Space
//! there: leaf instances live only in the database. A git-only restore point
//! of a database-backed Space would bring its folders back and leave every
//! database-only instance as it is now, so such a Space is never reverted
//! without its checkpoint.
//!
//! # The rules this module keeps
//!
//! 1. **A snapshot is all or nothing.** Save, checkpoint and commit run in one
//!    main-thread section under `GIT_COMMIT_LOCK`, so nothing writes the Space
//!    between them and the checkpoint matches its commit. A save that failed
//!    for any item, terrain that could not be written, or a failed commit
//!    leaves no snapshot behind (a checkpoint already written is deleted).
//!    When nothing changed since the last commit, the snapshot is an empty
//!    commit, so every checkpoint pairs with exactly one commit.
//! 2. **A revert never loses work.** It first takes a safety snapshot of the
//!    Space as it is, script tabs with unsaved edits included, and goes no
//!    further unless that snapshot was made. Reverting to the safety snapshot
//!    undoes the revert.
//! 3. **Nothing during Play.** Physics and scripts move things there and Stop
//!    restores them, so Play state is never an edit to snapshot or overwrite.
//!
//! Every snapshot and revert commit names who asked
//! (`Eustress-Requested-By`): `studio` for the person at this editor, and
//! `agent:<principal>` for a bridge caller, so an agent can never read as the
//! person. The git author is the signed-in user either way.
//!
//! The revert itself runs when the Space reopens (`run_pending_restore`, in
//! the world database's open step), because the database has to be closed to
//! be swapped. The restore and the reload run on the main thread, so the
//! editor is unresponsive while they do.

use bevy::prelude::*;
use std::path::{Path, PathBuf};

use crate::space::checkpoint;

/// The commit trailer that marks a snapshot and names it.
pub(crate) const TRAILER: &str = "Eustress-Snapshot";
/// Who asked for a snapshot or a revert.
const REQUESTED_BY: &str = "Eustress-Requested-By";
/// On a revert's safety snapshot: the snapshot the revert went to.
const SAFETY_FOR: &str = "Eustress-Safety-For";
/// On a revert's record: the snapshot it went to, and its safety snapshot.
const REVERTED_TO: &str = "Eustress-Reverted-To";
const SAFETY_SNAPSHOT: &str = "Eustress-Safety-Snapshot";
/// Database checkpoints kept per Space. Older snapshots stay in git; on a
/// database-backed Space they can no longer be reverted to.
pub(crate) const KEEP_CHECKPOINTS: usize = 20;
/// Commits `git log` searches for snapshot trailers.
pub(crate) const LOG_SCAN: usize = 5_000;
/// How long a revert waits for the reopened Space to finish its restore.
const REVERT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

// ---------------------------------------------------------------------------
// The plugin
// ---------------------------------------------------------------------------

/// The revert job and its pump, and the counter the History panel watches.
pub struct SnapshotPlugin;

impl Plugin for SnapshotPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RevertJob>()
            .init_resource::<SnapshotsChanged>()
            .add_systems(Update, pump_revert);
    }
}

/// Bumped whenever a snapshot, a revert or a cancel changes the Space's
/// restore points, whoever asked, so a view of them knows to refresh.
#[derive(Resource, Default)]
pub(crate) struct SnapshotsChanged(pub u64);

fn changed(world: &mut World) {
    if let Some(mut c) = world.get_resource_mut::<SnapshotsChanged>() {
        c.0 = c.0.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// Who asked
// ---------------------------------------------------------------------------

/// Who asked for a snapshot or a revert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The person at this Studio, through the History panel.
    Studio,
    /// A caller over the engine bridge, named by the principal it gave.
    Agent(String),
}

impl Origin {
    /// A bridge caller. The name is cleaned to one line; none given is "an
    /// agent".
    pub(crate) fn agent(principal: Option<&str>) -> Self {
        let name = clean_label(principal.unwrap_or(""));
        Origin::Agent(if name.is_empty() { "an agent".to_string() } else { name })
    }

    /// The `Eustress-Requested-By` value. An agent's is always prefixed, so
    /// no principal can read as `studio`.
    fn trailer(&self) -> String {
        match self {
            Origin::Studio => "studio".to_string(),
            Origin::Agent(who) => format!("agent:{who}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn space_root(world: &World) -> Result<PathBuf, String> {
    world
        .get_resource::<crate::space::SpaceRoot>()
        .map(|r| r.0.clone())
        .ok_or_else(|| "no Space is open".to_string())
}

fn in_play(world: &World) -> bool {
    world
        .get_resource::<State<crate::play_mode::PlayModeState>>()
        .is_some_and(|s| *s.get() != crate::play_mode::PlayModeState::Editing)
}

fn hosting(world: &World) -> bool {
    world
        .get_resource::<crate::multiplayer::HostRequest>()
        .is_some_and(|h| h.is_hosting())
}

const PLAY_REFUSAL: &str = "the editor is in Play. Stop first: Stop restores everything Play \
                            changed, so there is nothing from Play to snapshot or revert";
const PENDING_REFUSAL: &str = "a revert is waiting for this Space to reopen. Reopen the Space to \
                               apply it, or cancel it (Cancel in History, or \
                               cancel_pending_revert)";
const HOSTING_REFUSAL: &str = "this Space is being hosted (F9). Stop hosting first: the host \
                               holds its database open";

fn ensure_editing(world: &World) -> Result<(), String> {
    if in_play(world) { Err(PLAY_REFUSAL.to_string()) } else { Ok(()) }
}

fn ensure_no_pending_revert(space_root: &Path) -> Result<(), String> {
    if checkpoint::restore_pending(space_root) { Err(PENDING_REFUSAL.to_string()) } else { Ok(()) }
}

/// Why Save and Revert are refused right now, as short phrases for a view
/// that offers them; the same checks [`take_snapshot`] and [`start_revert`]
/// make, whose errors give the full reason. Each is `None` when allowed.
pub(crate) struct Blockers {
    pub save: Option<String>,
    pub revert: Option<String>,
    /// A revert waits for the Space to reopen; it can be cancelled.
    pub pending: bool,
    /// The label of the revert running now.
    pub busy: Option<String>,
}

pub(crate) fn blockers(world: &World) -> Blockers {
    let busy = world
        .get_resource::<RevertJob>()
        .and_then(|j| j.0.as_ref().map(|r| r.target_label.clone()));
    let Ok(root) = space_root(world) else {
        let none = Some("open a Space to use restore points".to_string());
        return Blockers { save: none.clone(), revert: none, pending: false, busy };
    };
    let pending = checkpoint::restore_pending(&root);
    let save = if busy.is_some() {
        Some("a revert is running".to_string())
    } else if in_play(world) {
        Some("stop Play to save or revert".to_string())
    } else if pending {
        Some("a revert is waiting for this Space to reopen".to_string())
    } else {
        None
    };
    let revert = save.clone().or_else(|| hosting(world).then(|| "stop hosting (F9) to revert".to_string()));
    Blockers { save, revert, pending, busy }
}

/// Run git in the Space with no console window, output captured.
pub(crate) fn git(space_root: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(args).current_dir(space_root);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if out.status.success() {
        Ok(out)
    } else {
        Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Split `-z` git output into paths.
pub(crate) fn nul_paths(out: &std::process::Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A new snapshot id: the local time to the nanosecond, so two snapshots in
/// one second still differ. Only `[0-9-]`, which the checkpoint store accepts.
fn new_snapshot_id() -> String {
    let now = chrono::Local::now();
    format!("{}-{:09}", now.format("%Y%m%d-%H%M%S"), now.timestamp_subsec_nanos())
}

/// One line, printable, at most 120 characters: a label goes into a commit
/// subject, where a newline would start the body and could forge a trailer.
pub(crate) fn clean_label(raw: &str) -> String {
    let one_line: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    one_line.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect()
}

// ---------------------------------------------------------------------------
// Taking a snapshot
// ---------------------------------------------------------------------------

pub(crate) struct Taken {
    pub id: String,
    pub sha: String,
    pub checkpoint: bool,
    pub written: usize,
    pub unchanged: usize,
    pub pruned: usize,
}

/// Whether taking a snapshot also prunes old database checkpoints. A revert's
/// safety snapshot does not: pruning could delete the checkpoint the revert
/// is about to restore.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Prune {
    Now,
    Later,
}

/// Save the Space and record it as snapshot `label`. See rule 1.
/// `safety_for` names the snapshot a revert is about to go to, when this is
/// that revert's safety snapshot.
pub(crate) fn take_snapshot(
    world: &mut World,
    label: &str,
    origin: &Origin,
    safety_for: Option<&str>,
    prune: Prune,
) -> Result<Taken, String> {
    use crate::ui::file_event_handler::{flush_space, FlushTerrain};

    let root = space_root(world)?;
    ensure_editing(world)?;
    ensure_no_pending_revert(&root)?;
    let db_backed = checkpoint::is_db_backed(&root);
    let id = new_snapshot_id();

    let flush = flush_space(world, FlushTerrain::Save)?;
    if flush.report.errors > 0 {
        return Err(format!(
            "{} item(s) failed to save, so no snapshot was made. The Output panel names them",
            flush.report.errors
        ));
    }
    if flush.terrain_unsaved {
        return Err("the terrain could not be saved, so no snapshot was made. The Output \
                    panel says why"
            .to_string());
    }

    let mut message = format!("snapshot: {label}\n\n{TRAILER}: {id}\n{REQUESTED_BY}: {}\n", origin.trailer());
    if let Some(target) = safety_for {
        message.push_str(&format!("{SAFETY_FOR}: {target}\n"));
    }
    let sha = {
        let _commit_guard = crate::editor_settings::GIT_COMMIT_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if db_backed {
            #[cfg(feature = "world-db")]
            {
                checkpoint::checkpoint_db(&root, &id, &flush.report.paths)
                    .map_err(|e| format!("database checkpoint failed, so no snapshot was made: {e}"))?;
            }
            #[cfg(not(feature = "world-db"))]
            {
                return Err("this Space keeps state in its database, and this build has no \
                            database support to checkpoint it, so no snapshot was made"
                    .to_string());
            }
        }
        let committed = crate::editor_settings::git_commit_locked(
            &root,
            &message,
            flush.identity.as_ref(),
            crate::editor_settings::EmptyCommit::Allow,
        );
        match committed {
            Ok(crate::editor_settings::GitAutosave::Committed(sha)) => sha,
            other => {
                if db_backed {
                    checkpoint::delete_checkpoint(&root, &id);
                }
                return Err(match other {
                    Err(e) => format!("the git commit failed, so no snapshot was made: {e}"),
                    _ => "git made no commit, so no snapshot was made".to_string(),
                });
            }
        }
    };

    let pruned = if db_backed && prune == Prune::Now {
        checkpoint::prune_checkpoints(&root, KEEP_CHECKPOINTS)
    } else {
        0
    };
    if let Some(mut state) = world.get_resource_mut::<crate::ui::StudioState>() {
        state.snapshot_status = format!("Snapshot {}", chrono::Local::now().format("%H:%M"));
    }
    changed(world);
    Ok(Taken {
        id,
        sha,
        checkpoint: db_backed,
        written: flush.report.written,
        unchanged: flush.report.unchanged,
        pruned,
    })
}

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

pub(crate) struct Listed {
    pub id: String,
    pub sha: String,
    pub created: String,
    pub label: String,
}

/// Whether the Space has a repository with at least one commit; `git log`
/// fails on one without.
fn has_history(root: &Path) -> bool {
    root.join(".git").exists() && git(root, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok()
}

/// Snapshots on the current branch, newest first.
pub(crate) fn list(root: &Path, limit: usize) -> Result<Vec<Listed>, String> {
    if !has_history(root) {
        return Ok(Vec::new());
    }
    let format = format!("--format=%H%x1f%cI%x1f%s%x1f%(trailers:key={TRAILER},valueonly,separator=%x2c)%x1e");
    let grep = format!("--grep=^{TRAILER}: ");
    let scan = format!("-n{LOG_SCAN}");
    let out = git(root, &["log", &scan, &grep, &format])?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut listed = Vec::new();
    for record in text.split('\u{1e}') {
        let fields: Vec<&str> = record.trim_matches(['\n', '\r']).split('\u{1f}').collect();
        let [sha, created, subject, trailer] = fields[..] else { continue };
        // A commit whose trailer names more than one id is not one this
        // module wrote; skip it rather than guess.
        let id = trailer.trim();
        if id.is_empty() || id.contains(',') {
            continue;
        }
        listed.push(Listed {
            id: id.to_string(),
            sha: sha.to_string(),
            created: created.to_string(),
            label: subject.strip_prefix("snapshot: ").unwrap_or(subject).to_string(),
        });
        if listed.len() >= limit {
            break;
        }
    }
    Ok(listed)
}

pub(crate) fn find(root: &Path, id: &str) -> Result<Listed, String> {
    list(root, LOG_SCAN)?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("no snapshot '{id}' on this Space's current branch"))
}

/// What a row of the Space's restore-point history is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PointKind {
    /// A snapshot someone asked for.
    Snapshot,
    /// The snapshot a revert took of the Space just before it.
    Safety,
    /// A revert's record. `failed` when it was withdrawn.
    Revert { failed: bool },
}

/// One row of the Space's restore-point history.
#[derive(Clone, Debug)]
pub(crate) struct RestorePoint {
    pub kind: PointKind,
    /// The snapshot's id; for a revert record, the snapshot it went to.
    pub id: String,
    /// When the commit was made, RFC 3339.
    pub created: String,
    pub label: String,
    /// The commit's author: the signed-in user.
    pub author: String,
    /// True when the person at Studio asked; false for an agent, or for an
    /// older commit that does not say.
    pub by_studio: bool,
    /// The agent's name, when an agent asked.
    pub agent: Option<String>,
    /// For a revert record: its safety snapshot, which undoes it.
    pub safety: Option<String>,
    pub revertible: bool,
    /// Why a snapshot cannot be reverted to, or a revert's outcome.
    pub note: String,
}

fn one_value(field: &str) -> Option<String> {
    let v = field.trim();
    (!v.is_empty() && !v.contains(',')).then(|| v.to_string())
}

/// The Space's snapshots and revert records, newest first.
pub(crate) fn history(root: &Path, limit: usize) -> Result<Vec<RestorePoint>, String> {
    if !has_history(root) {
        return Ok(Vec::new());
    }
    let format = format!(
        "--format=%H%x1f%cI%x1f%an%x1f%s%x1f\
         %(trailers:key={TRAILER},valueonly,separator=%x2c)%x1f\
         %(trailers:key={REQUESTED_BY},valueonly,separator=%x2c)%x1f\
         %(trailers:key={SAFETY_FOR},valueonly,separator=%x2c)%x1f\
         %(trailers:key={REVERTED_TO},valueonly,separator=%x2c)%x1f\
         %(trailers:key={SAFETY_SNAPSHOT},valueonly,separator=%x2c)%x1e"
    );
    let scan = format!("-n{LOG_SCAN}");
    let snapshots = format!("--grep=^{TRAILER}: ");
    let reverts = format!("--grep=^{REVERTED_TO}: ");
    let out = git(root, &["log", &scan, &snapshots, &reverts, &format])?;

    let db_backed = checkpoint::is_db_backed(root);
    let kept: std::collections::HashSet<String> = if db_backed {
        checkpoint::list_checkpoints(root).into_iter().map(|c| c.id).collect()
    } else {
        Default::default()
    };

    let text = String::from_utf8_lossy(&out.stdout);
    let mut rows = Vec::new();
    for record in text.split('\u{1e}') {
        let fields: Vec<&str> = record.trim_matches(['\n', '\r']).split('\u{1f}').collect();
        let [_sha, created, author, subject, snap, by, safety_for, reverted_to, safety] = fields[..] else {
            continue;
        };
        let by = one_value(by);
        let by_studio = by.as_deref() == Some("studio");
        let agent = by
            .filter(|b| b != "studio")
            .map(|b| b.strip_prefix("agent:").unwrap_or(&b).to_string());

        if let Some(target) = one_value(reverted_to) {
            let failed = subject.ends_with(" failed");
            let safety = one_value(safety);
            rows.push(RestorePoint {
                kind: PointKind::Revert { failed },
                id: target,
                created: created.to_string(),
                label: subject.to_string(),
                author: author.to_string(),
                by_studio,
                agent,
                note: match (failed, &safety) {
                    (true, _) => "withdrawn; nothing changed".to_string(),
                    (false, Some(s)) => format!("undo: revert to {s}"),
                    (false, None) => String::new(),
                },
                safety,
                revertible: false,
            });
        } else if let Some(id) = one_value(snap) {
            let label = subject.strip_prefix("snapshot: ").unwrap_or(subject).to_string();
            // Older safety snapshots carry no trailer, only their label.
            let is_safety = one_value(safety_for).is_some() || label.starts_with("before revert to ");
            let revertible = !db_backed || kept.contains(&id);
            rows.push(RestorePoint {
                kind: if is_safety { PointKind::Safety } else { PointKind::Snapshot },
                note: if revertible {
                    String::new()
                } else {
                    format!("database checkpoint pruned (the newest {KEEP_CHECKPOINTS} are kept)")
                },
                id,
                created: created.to_string(),
                label,
                author: author.to_string(),
                by_studio,
                agent,
                safety: None,
                revertible,
            });
        }
        if rows.len() >= limit {
            break;
        }
    }
    // A revert record names its safety snapshot by id; say it by label, the
    // way the list shows it. A safety snapshot is older than its record, so a
    // cut-off list may not hold it, and then the id stands.
    let labels: std::collections::HashMap<String, String> = rows
        .iter()
        .filter(|r| !matches!(r.kind, PointKind::Revert { .. }))
        .map(|r| (r.id.clone(), r.label.clone()))
        .collect();
    for row in rows.iter_mut() {
        if let (PointKind::Revert { failed: false }, Some(safety)) = (&row.kind, &row.safety) {
            let shown = labels.get(safety).cloned().unwrap_or_else(|| safety.clone());
            row.note = format!("undo: revert to '{shown}'");
        }
    }
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Reverting
// ---------------------------------------------------------------------------

/// How a revert ended, handed to whoever asked for it.
pub(crate) struct RevertOutcome {
    pub target: String,
    pub target_label: String,
    /// The safety snapshot, which undoes the revert.
    pub safety: String,
    pub paths: Option<Vec<String>>,
    pub result: Result<RevertDone, String>,
}

pub(crate) struct RevertDone {
    /// The commit recording the revert, when it could be made.
    pub audit_commit: Option<String>,
    /// True only when the wait timed out with the load unsettled.
    pub still_loading: bool,
}

/// Called once with the outcome, on the main thread.
pub(crate) type RevertReply = Box<dyn FnOnce(&mut World, &RevertOutcome) + Send + Sync>;

/// A revert waiting for the reopened Space to finish its restore.
struct InflightRevert {
    root: PathBuf,
    target: String,
    target_label: String,
    safety: String,
    paths: Option<Vec<String>>,
    origin: Origin,
    started: std::time::Instant,
    reply: RevertReply,
}

/// The single in-flight revert, if any.
#[derive(Resource, Default)]
pub(crate) struct RevertJob(Option<InflightRevert>);

/// A path to revert: relative to the Space, inside it, and not git's own.
pub(crate) fn check_path(raw: &str) -> Result<String, String> {
    let path = raw.trim().replace('\\', "/");
    let path = path.trim_end_matches('/');
    let p = Path::new(path);
    let bad = path.is_empty()
        || p.is_absolute()
        || p.has_root()
        || path.contains(':')
        || p.components().any(|c| !matches!(c, std::path::Component::Normal(_)))
        || path == ".git"
        || path.starts_with(".git/");
    if bad {
        Err(format!(
            "'{raw}' is not a path inside the Space. Give paths relative to the Space folder, \
             such as Workspace/Tower"
        ))
    } else {
        Ok(path.to_string())
    }
}

/// Files the target snapshot holds (within `paths`) that are on disk now but
/// ignored by git. The restore would overwrite them, and the safety snapshot,
/// being a commit, cannot hold an ignored file, so the revert refuses.
fn ignored_files_the_revert_would_overwrite(
    root: &Path,
    sha: &str,
    paths: Option<&[String]>,
) -> Result<Vec<String>, String> {
    let scope: Vec<&str> = paths.map(|p| p.iter().map(String::as_str).collect()).unwrap_or_default();
    let mut tree_args = vec!["ls-tree", "-r", "--name-only", "-z", sha, "--"];
    tree_args.extend(&scope);
    let tracked = nul_paths(&git(root, &tree_args)?);

    // `--directory` names an ignored folder once, with a trailing slash,
    // rather than every file in it (world.fjalldb/ alone is thousands).
    let mut ignored_args =
        vec!["ls-files", "--others", "--ignored", "--exclude-standard", "--directory", "-z", "--"];
    ignored_args.extend(&scope);
    let ignored = nul_paths(&git(root, &ignored_args)?);
    let (dirs, files): (Vec<&String>, Vec<&String>) = ignored.iter().partition(|p| p.ends_with('/'));
    let files: std::collections::HashSet<&str> = files.into_iter().map(String::as_str).collect();

    // The engine's runtime files are rewritten on every open, so their
    // current value needs no safety copy.
    let runtime = crate::editor_settings::RUNTIME_STATE;
    Ok(tracked
        .into_iter()
        .filter(|t| !runtime.contains(&t.as_str()))
        .filter(|t| files.contains(t.as_str()) || dirs.iter().any(|d| t.starts_with(d.as_str())))
        .collect())
}

/// Start a revert to snapshot `target`: validate, save the script tabs, take
/// the safety snapshot, record the restore plan, and reopen the Space to run
/// it. `reply` gets the outcome once the reopened Space has applied or
/// withdrawn the plan. An `Err` here means nothing started and `reply` is
/// never called.
pub(crate) fn start_revert(
    world: &mut World,
    target: &str,
    paths: Option<&[String]>,
    origin: Origin,
    reply: RevertReply,
) -> Result<(), String> {
    if world.get_resource::<RevertJob>().is_some_and(|j| j.0.is_some()) {
        return Err("a revert is already running. Wait for it to finish".to_string());
    }
    let root = space_root(world)?;
    ensure_editing(world)?;
    ensure_no_pending_revert(&root)?;
    // A host holds the database open, so the restore could not swap it.
    if hosting(world) {
        return Err(HOSTING_REFUSAL.to_string());
    }
    let snap = find(&root, target)?;
    let paths = match paths {
        None => None,
        Some(p) if p.is_empty() => None,
        Some(p) => Some(p.iter().map(|s| check_path(s)).collect::<Result<Vec<_>, _>>()?),
    };

    let db_backed = checkpoint::is_db_backed(&root);
    if db_backed {
        if paths.is_some() {
            return Err("this Space keeps instances in its database, which reverts as a whole. \
                        Leave out 'paths' to revert the whole Space"
                .to_string());
        }
        if !checkpoint::list_checkpoints(&root).iter().any(|c| c.id == snap.id) {
            return Err(format!(
                "snapshot '{}' has no database checkpoint any more (the newest {KEEP_CHECKPOINTS} \
                 are kept), and this Space keeps instances in its database, so reverting its \
                 files alone would leave those instances as they are now. Choose a revertible \
                 snapshot",
                snap.label
            ));
        }
    }

    let exposed = ignored_files_the_revert_would_overwrite(&root, &snap.sha, paths.as_deref())?;
    if !exposed.is_empty() {
        let shown: Vec<&str> = exposed.iter().take(20).map(String::as_str).collect();
        return Err(format!(
            "snapshot '{}' holds {} file(s) that are on disk now but ignored by git, so the \
             safety snapshot could not keep them and the revert would overwrite them: {}{}. \
             Move them aside, then revert again",
            snap.label,
            exposed.len(),
            shown.join(", "),
            if exposed.len() > shown.len() { ", ..." } else { "" }
        ));
    }

    // Rule 2. The reopen below saves dirty script tabs itself, after the
    // safety snapshot and just before the restore overwrites the files, so
    // they are saved here, where the safety snapshot includes them.
    save_dirty_code_tabs(world).map_err(|e| format!("the revert did not start: {e}"))?;
    let safety = take_snapshot(world, &format!("before revert to '{}'", snap.label), &origin, Some(&snap.id), Prune::Later)
        .map_err(|e| format!("the revert did not start: {e}"))?;

    checkpoint::request_restore(
        &root,
        &checkpoint::RestorePlan {
            git_sha: snap.sha.clone(),
            paths: paths.clone(),
            checkpoint_id: db_backed.then(|| snap.id.clone()),
        },
    )
    .map_err(|e| format!("the revert did not start: {e}. Nothing changed; safety snapshot {} was saved", safety.id))?;

    world.resource_mut::<RevertJob>().0 = Some(InflightRevert {
        root: root.clone(),
        target: snap.id,
        target_label: snap.label,
        safety: safety.id,
        paths,
        origin,
        started: std::time::Instant::now(),
        reply,
    });
    crate::space::space_ops::open_space(world, &root);
    Ok(())
}

/// Finish the in-flight revert once the reopened Space has applied the plan
/// or withdrawn it: record it, tell the person at the editor, and answer
/// whoever asked.
fn pump_revert(world: &mut World) {
    let Some(job) = world.get_resource_mut::<RevertJob>().and_then(|mut j| j.0.take()) else {
        return;
    };
    // The restore runs in the first Update after the reopen, and then the
    // Space loads over many frames. `open_space` marks the load begun before
    // it returns, so neither check reads settled too early.
    let pending = checkpoint::restore_pending(&job.root);
    let loading = world
        .get_resource::<crate::space::file_loader::LoadInProgress>()
        .map_or(false, |l| l.active);
    if (pending || loading) && job.started.elapsed() < REVERT_TIMEOUT {
        world.resource_mut::<RevertJob>().0 = Some(job);
        return;
    }

    let InflightRevert { root, target, target_label, safety, paths, origin, reply, .. } = job;
    let outcome = if pending {
        RevertOutcome {
            result: Err(format!(
                "the revert to '{target_label}' has not run after {} s. It applies when the Space \
                 next opens, or cancel it. Safety snapshot {safety} holds the Space from before",
                REVERT_TIMEOUT.as_secs()
            )),
            target,
            target_label,
            safety,
            paths,
        }
    } else {
        // The safety snapshot's checkpoint was kept out of pruning until the
        // restore no longer needed the target's.
        if checkpoint::is_db_backed(&root) {
            checkpoint::prune_checkpoints(&root, KEEP_CHECKPOINTS);
        }
        let failure = checkpoint::last_restore_failure(&root);
        let audit = audit_commit(world, &root, &target, &safety, paths.as_deref(), &origin, failure.as_deref());
        if let Err(ref e) = audit {
            warn!("Revert to snapshot {target}: the audit commit failed: {e}");
        }
        notify(world, &origin, &target_label, failure.as_deref());
        RevertOutcome {
            result: match failure {
                Some(reason) => Err(format!(
                    "the revert to '{target_label}' failed: {reason}. Nothing changed; the Space \
                     reopened as it was. Safety snapshot {safety} was saved"
                )),
                None => Ok(RevertDone { audit_commit: audit.ok(), still_loading: loading }),
            },
            target,
            target_label,
            safety,
            paths,
        }
    };
    changed(world);
    reply(world, &outcome);
}

/// Tell the person at the editor what a finished revert did. An agent's
/// revert changes the Space under them, so it is a warning that says how to
/// undo it.
fn notify(world: &mut World, origin: &Origin, label: &str, failure: Option<&str>) {
    use crate::notifications::NotificationManager;
    let Some(mut n) = world.get_resource_mut::<NotificationManager>() else { return };
    match (origin, failure) {
        (Origin::Studio, None) => n.success(format!(
            "Restored '{label}'. Your work from just before is a restore point in History; revert \
             to it to undo."
        )),
        (Origin::Studio, Some(reason)) => n.error(format!("Revert to '{label}' failed: {reason}. Nothing changed.")),
        (Origin::Agent(who), None) => n.warning(format!(
            "An agent ({who}) restored '{label}'. Your work from just before is a restore point in \
             History; revert to it to undo."
        )),
        (Origin::Agent(who), Some(reason)) => n.warning(format!(
            "An agent's ({who}) revert to '{label}' failed: {reason}. Nothing changed."
        )),
    }
    match failure {
        Some(_) => warn!("Revert to '{label}' by {} failed.", origin.trailer()),
        None => info!("Reverted to '{label}' for {}.", origin.trailer()),
    }
}

/// Record a finished revert in the Space's history: a commit of the Space as
/// the revert left it (empty when nothing changed, as after a failure), whose
/// trailers name the snapshot, the safety snapshot and who asked. The git
/// author is the signed-in user.
fn audit_commit(
    world: &World,
    root: &Path,
    target: &str,
    safety: &str,
    paths: Option<&[String]>,
    origin: &Origin,
    failure: Option<&str>,
) -> Result<String, String> {
    let identity = world
        .get_resource::<crate::auth::AuthState>()
        .and_then(|auth| crate::editor_settings::git_identity_from_auth(auth));
    let mut message = match failure {
        None => format!("revert to snapshot {target}\n\n"),
        Some(reason) => format!("revert to snapshot {target} failed\n\n{}\n\n", clean_label(reason)),
    };
    if let Some(paths) = paths {
        message.push_str(&format!("Paths: {}\n\n", paths.join(", ")));
    }
    message.push_str(&format!(
        "{REVERTED_TO}: {target}\n{SAFETY_SNAPSHOT}: {safety}\n{REQUESTED_BY}: {}\n",
        origin.trailer()
    ));
    let _commit_guard = crate::editor_settings::GIT_COMMIT_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    match crate::editor_settings::git_commit_locked(
        root,
        &message,
        identity.as_ref(),
        crate::editor_settings::EmptyCommit::Allow,
    )? {
        crate::editor_settings::GitAutosave::Committed(sha) => Ok(sha),
        crate::editor_settings::GitAutosave::NoChanges => Err("git made no commit".to_string()),
    }
}

/// Withdraw a revert waiting for the Space to reopen. True when one waited.
pub(crate) fn cancel_pending(world: &mut World) -> Result<bool, String> {
    let root = space_root(world)?;
    let cancelled = checkpoint::cancel_restore(&root);
    changed(world);
    Ok(cancelled)
}

// ---------------------------------------------------------------------------
// Script tabs
// ---------------------------------------------------------------------------

pub(crate) fn dirty_code_tab_names(world: &World) -> Vec<String> {
    world
        .get_resource::<crate::ui::center_tabs::CenterTabManager>()
        .map(|m| m.tabs.iter().filter(|t| t.dirty).map(|t| t.name.clone()).collect())
        .unwrap_or_default()
}

/// Save every script tab with unsaved edits, as switching Space does. Any
/// failure stops the caller: a revert would overwrite that file.
fn save_dirty_code_tabs(world: &mut World) -> Result<(), String> {
    let failed = crate::ui::center_tabs::save_dirty_code_tabs(world);
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("script tab(s) with unsaved edits could not be saved: {}", failed.join("; ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_can_never_read_as_the_person_at_studio() {
        assert_eq!(Origin::Studio.trailer(), "studio");
        assert_eq!(Origin::agent(Some("studio")).trailer(), "agent:studio");
        assert_eq!(Origin::agent(Some("mcp-client")).trailer(), "agent:mcp-client");
        assert_eq!(Origin::agent(None).trailer(), "agent:an agent");
        assert_eq!(Origin::agent(Some("  \n ")).trailer(), "agent:an agent");
    }

    #[test]
    fn a_label_cannot_start_a_trailer() {
        assert_eq!(clean_label("fine\nEustress-Snapshot: forged"), "fine Eustress-Snapshot: forged");
        assert_eq!(clean_label(&"x".repeat(300)).len(), 120);
    }

    #[test]
    fn revert_paths_stay_inside_the_space() {
        assert_eq!(check_path("Workspace\\Tower/").unwrap(), "Workspace/Tower");
        for bad in ["", "/etc", "C:/x", "../up", "Workspace/../../x", ".git", ".git/config"] {
            assert!(check_path(bad).is_err(), "{bad}");
        }
    }
}
