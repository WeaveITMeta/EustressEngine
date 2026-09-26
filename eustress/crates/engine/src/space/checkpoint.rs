//! Snapshots of a Space's database, and the revert that puts one back.
//!
//! A snapshot is a git commit of the Space folder plus, for a Space whose
//! state lives in its database, a checkpoint of that database
//! ([`eustress_worlddb::checkpoint`]): every partition at one sequence number,
//! taken while the Space is open. Checkpoints live outside git, in
//! `.eustress/snapshots/<id>.ewck`, listed in `.eustress/snapshots/index.toml`;
//! a waiting revert and the last failure sit there too, all ignored by git.
//! The tree's copies of files git versions (meshes, images, audio, terrain
//! rasters: every kind `representation::tree_tracks` leaves out) are stored as
//! a key and a hash, and a restore takes their bytes from the snapshot's git
//! commit.
//!
//! A revert runs while the Space is CLOSED, in the open path. The caller
//! writes a [`RestorePlan`] with [`request_restore`] and reopens the Space.
//! Before the database opens, [`run_pending_restore`] builds the checkpoint
//! into a fresh `world.fjalldb.restoring`, swaps it in for the live database
//! (kept as `world.fjalldb.pre-restore-<id>`), then puts the files back from
//! git, all under the git commit lock. While a restore is pending, saves
//! refuse and the file watcher drops its events, so nothing writes the
//! outgoing World over the files being restored. A revert that fails puts
//! both halves back (the database set aside, the files from the safety
//! snapshot at HEAD) and is withdrawn, its reason kept for the caller
//! ([`last_restore_failure`]), so a failure never leaves saves refused. Only
//! a process that dies mid-revert leaves the plan, and the next open runs it.

use std::path::{Path, PathBuf};

/// Where a Space's checkpoints live.
pub fn snapshots_dir(space_root: &Path) -> PathBuf {
    space_root.join(".eustress").join("snapshots")
}

// The plan and the failure record sit in the snapshots folder, which git
// ignores: committed, a later revert would write an old one back.
fn marker_path(space_root: &Path) -> PathBuf {
    snapshots_dir(space_root).join("restore_pending.toml")
}

fn failure_path(space_root: &Path) -> PathBuf {
    snapshots_dir(space_root).join("restore_failed.toml")
}

/// Why the last revert of this Space failed, when it did. Cleared by the
/// next [`request_restore`].
pub fn last_restore_failure(space_root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(failure_path(space_root)).ok()?;
    let doc: toml::Value = text.parse().ok()?;
    doc.get("reason").and_then(|r| r.as_str()).map(str::to_string)
}

fn checkpoint_file(space_root: &Path, id: &str) -> PathBuf {
    snapshots_dir(space_root).join(format!("{id}.ewck"))
}

fn index_file(space_root: &Path) -> PathBuf {
    snapshots_dir(space_root).join("index.toml")
}

/// True while a revert waits for this Space to reopen. Saves refuse then:
/// anything they wrote would be restored over, or worse, land after it.
pub fn restore_pending(space_root: &Path) -> bool {
    marker_path(space_root).is_file()
}

/// True when part of the Space's state lives only in its database, so a git
/// commit of the folder alone is not a whole snapshot.
pub fn is_db_backed(space_root: &Path) -> bool {
    space_root.join("world.fjalldb").is_dir()
}

/// A checkpoint on disk.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Checkpoint {
    pub id: String,
    /// RFC 3339.
    pub created: String,
    /// The sequence number every partition was read at.
    pub seqno: u64,
    pub bytes: u64,
    /// Records stored as a key and a hash, their bytes taken from git.
    pub elided: u64,
    /// Records per partition.
    pub partitions: Vec<(String, u64)>,
}

/// What a revert puts back, and from where.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RestorePlan {
    /// The snapshot's commit.
    pub git_sha: String,
    /// Space-relative paths to revert; `None` reverts the whole Space.
    pub paths: Option<Vec<String>>,
    /// The snapshot's checkpoint; `None` for a Space with no database.
    pub checkpoint_id: Option<String>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Index {
    #[serde(default)]
    checkpoint: Vec<Checkpoint>,
}

fn valid_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(format!("checkpoint id `{id}` must be 1 to 64 letters, digits, `-` or `_`"))
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn read_index(space_root: &Path) -> Index {
    std::fs::read_to_string(index_file(space_root))
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_index(space_root: &Path, index: &Index) -> Result<(), String> {
    let text = toml::to_string(index).map_err(|e| format!("snapshot index: {e}"))?;
    write_atomic(&index_file(space_root), text.as_bytes())
}

/// The Space's checkpoints, oldest first. An entry whose file is gone is left out.
pub fn list_checkpoints(space_root: &Path) -> Vec<Checkpoint> {
    let mut list: Vec<Checkpoint> = read_index(space_root)
        .checkpoint
        .into_iter()
        .filter(|c| checkpoint_file(space_root, &c.id).is_file())
        .collect();
    list.sort_by(|a, b| a.created.cmp(&b.created));
    list
}

/// Delete checkpoint `id`: its file and its index entry. True when its file
/// was there to delete.
pub fn delete_checkpoint(space_root: &Path, id: &str) -> bool {
    if valid_id(id).is_err() {
        return false;
    }
    let removed = std::fs::remove_file(checkpoint_file(space_root, id)).is_ok();
    let mut index = read_index(space_root);
    let before = index.checkpoint.len();
    index.checkpoint.retain(|c| c.id != id);
    if index.checkpoint.len() != before {
        if let Err(e) = write_index(space_root, &index) {
            tracing::warn!(target: "eustress_engine::checkpoint", "delete {id}: {e}");
        }
    }
    removed
}

/// Delete all but the newest `keep_newest` checkpoints. Returns how many went.
pub fn prune_checkpoints(space_root: &Path, keep_newest: usize) -> usize {
    let list = list_checkpoints(space_root);
    let excess = list.len().saturating_sub(keep_newest);
    let mut removed = 0;
    for c in &list[..excess] {
        if std::fs::remove_file(checkpoint_file(space_root, &c.id)).is_ok() {
            removed += 1;
        }
    }
    let kept = Index { checkpoint: list[excess..].to_vec() };
    if let Err(e) = write_index(space_root, &kept) {
        tracing::warn!(target: "eustress_engine::checkpoint", "prune: {e}");
    }
    removed
}

/// Ask for a revert. It runs the next time this Space opens, before its
/// database does ([`run_pending_restore`]); the caller reopens the Space.
///
/// A Space with a database needs its checkpoint: a revert of the files alone
/// would report success and leave most of the world as it was. Reverting
/// only some paths of such a Space waits for the prefix restore.
pub fn request_restore(space_root: &Path, plan: &RestorePlan) -> Result<(), String> {
    let sha = plan.git_sha.trim();
    if sha.is_empty() || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("`{}` is not a commit id", plan.git_sha));
    }
    match &plan.checkpoint_id {
        Some(id) => {
            valid_id(id)?;
            if !checkpoint_file(space_root, id).is_file() {
                return Err(format!("checkpoint {id} is missing, so this snapshot cannot be restored"));
            }
            if plan.paths.is_some() {
                return Err("per-path revert on this Space lands with the prefix restore".to_string());
            }
        }
        None if is_db_backed(space_root) => {
            return Err("this Space keeps state in its database; a revert needs the snapshot's checkpoint".to_string());
        }
        None => {}
    }
    let text = toml::to_string(plan).map_err(|e| format!("restore plan: {e}"))?;
    let _ = std::fs::remove_file(failure_path(space_root));
    write_atomic(&marker_path(space_root), text.as_bytes())
}

/// Withdraw a waiting revert. A plan that keeps failing is otherwise tried at
/// every open, and saves refuse while it waits. True when one was waiting.
pub fn cancel_restore(space_root: &Path) -> bool {
    std::fs::remove_file(marker_path(space_root)).is_ok()
}

// ── The watcher's quiet window ─────────────────────────────────────────────

static QUIET_UNTIL: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// True while the file watcher must drop its events: a revert just rewrote
/// the Space's files, and the reopen reads all of them.
pub fn watcher_quiet() -> bool {
    let guard = QUIET_UNTIL.lock().unwrap_or_else(|p| p.into_inner());
    guard.is_some_and(|until| std::time::Instant::now() < until)
}

#[cfg(feature = "world-db")]
fn quiet_for(duration: std::time::Duration) {
    let mut guard = QUIET_UNTIL.lock().unwrap_or_else(|p| p.into_inner());
    *guard = Some(std::time::Instant::now() + duration);
}

// ── Git ────────────────────────────────────────────────────────────────────

#[cfg(feature = "world-db")]
fn git(space_root: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(space_root);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[cfg(feature = "world-db")]
fn run_git(space_root: &Path, args: &[&str]) -> Result<String, String> {
    let out = git(space_root)
        .args(args)
        .output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// The pathspec a revert covers: its paths, or the whole Space, never the
/// engine's runtime state (`editor_settings::RUNTIME_STATE`). The engine
/// rewrites those files on every open, and a snapshot taken before git
/// ignored them still holds old values a revert would write back.
#[cfg(feature = "world-db")]
fn scope_of(paths: Option<&[String]>) -> Vec<String> {
    let mut scope: Vec<String> = match paths {
        Some(paths) => paths.to_vec(),
        None => vec![".".to_string()],
    };
    scope.extend(
        crate::editor_settings::RUNTIME_STATE
            .iter()
            .map(|entry| format!(":(exclude){entry}")),
    );
    scope
}

/// Put the files under `paths` (the whole Space when `None`) back as commit
/// `sha` had them. `git restore` also removes the tracked files the commit
/// lacks, within the pathspec; the folders those leave empty go too, since an
/// empty folder loads as a Folder. Nothing outside the pathspec is touched:
/// reverting one Model never deletes new work elsewhere.
#[cfg(feature = "world-db")]
fn restore_files(space_root: &Path, sha: &str, paths: Option<&[String]>) -> Result<(), String> {
    let scope = scope_of(paths);
    let mut added = vec!["diff", "--name-only", "--diff-filter=A", "-z", sha, "HEAD", "--"];
    added.extend(scope.iter().map(String::as_str));
    let added = run_git(space_root, &added)?;
    let mut restore = vec!["restore", "--source", sha, "--worktree", "--"];
    restore.extend(scope.iter().map(String::as_str));
    run_git(space_root, &restore)?;
    for rel in added.split('\0').filter(|p| !p.is_empty()) {
        let file = space_root.join(rel);
        // Already gone with a current git; an older one leaves it.
        let _ = std::fs::remove_file(&file);
        let mut dir = file.parent();
        while let Some(d) = dir.filter(|d| *d != space_root && d.starts_with(space_root)) {
            if std::fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
    }
    Ok(())
}

/// Reads file contents out of one commit through a single `git cat-file`.
#[cfg(feature = "world-db")]
struct CommitFiles {
    child: std::process::Child,
    input: std::process::ChildStdin,
    output: std::io::BufReader<std::process::ChildStdout>,
    sha: String,
}

#[cfg(feature = "world-db")]
impl CommitFiles {
    fn open(space_root: &Path, sha: &str) -> Result<Self, String> {
        let mut child = git(space_root)
            .args(["cat-file", "--batch"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("git cat-file: {e}"))?;
        let input = child.stdin.take().ok_or("git cat-file: no stdin")?;
        let output = std::io::BufReader::new(child.stdout.take().ok_or("git cat-file: no stdout")?);
        Ok(Self { child, input, output, sha: sha.to_string() })
    }

    /// The file at `rel` in the commit, or `None` when the commit lacks it.
    fn read(&mut self, rel: &str) -> Option<Vec<u8>> {
        use std::io::{BufRead, Read, Write};
        writeln!(self.input, "{}:{}", self.sha, rel).ok()?;
        self.input.flush().ok()?;
        let mut header = String::new();
        self.output.read_line(&mut header).ok()?;
        // "<oid> <type> <size>", or "<name> missing".
        let mut fields = header.split_whitespace();
        let (_oid, kind, size) = (fields.next()?, fields.next()?, fields.next()?);
        let size: usize = size.parse().ok()?;
        let mut bytes = vec![0u8; size];
        self.output.read_exact(&mut bytes).ok()?;
        let mut newline = [0u8; 1];
        self.output.read_exact(&mut newline).ok()?;
        (kind == "blob").then_some(bytes)
    }
}

#[cfg(feature = "world-db")]
impl Drop for CommitFiles {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(feature = "world-db")]
pub use with_db::*;

#[cfg(feature = "world-db")]
mod with_db {
    use super::*;
    use eustress_worlddb::RestoreSummary;

    /// Checkpoint the Space's open database as `id`. Call it on the main
    /// thread straight after a flush, before the snapshot's commit; `written`
    /// are the files that flush wrote, put into the tree first because the
    /// file watcher brings disk writes into it up to a second late.
    pub fn checkpoint_db(space_root: &Path, id: &str, written: &[PathBuf]) -> Result<Checkpoint, String> {
        valid_id(id)?;
        let db = crate::space::active_db::db_arc().ok_or("no database is open")?;
        if crate::space::active_db::root().as_deref() != Some(space_root) {
            return Err(format!("the open database is not {}'s", space_root.display()));
        }
        let out = checkpoint_file(space_root, id);
        if out.exists() {
            return Err(format!("checkpoint {id} already exists"));
        }
        for path in written {
            if let Ok(bytes) = std::fs::read(path) {
                crate::space::active_db::put_tree_file(path, &bytes);
            }
        }
        // A file git versions keeps only its key and hash, when the file is
        // on disk to be committed beside the checkpoint. `#bin` twins live
        // only in the database and are always kept whole.
        let root = space_root.to_path_buf();
        let elide = move |partition: &str, key: &[u8]| -> bool {
            partition == "tree"
                && !key.ends_with(b"#bin")
                && std::str::from_utf8(key).is_ok_and(|rel| {
                    !crate::space::representation::tree_tracks(rel) && root.join(rel).is_file()
                })
        };
        let info = db.checkpoint_to(&out, &elide).map_err(|e| format!("checkpoint {id}: {e}"))?;
        let checkpoint = Checkpoint {
            id: id.to_string(),
            created: chrono::Utc::now().to_rfc3339(),
            seqno: info.seqno,
            bytes: info.bytes,
            elided: info.elided,
            partitions: info.partitions,
        };
        let mut index = read_index(space_root);
        index.checkpoint.retain(|c| c.id != checkpoint.id);
        index.checkpoint.push(checkpoint.clone());
        write_index(space_root, &index)?;
        Ok(checkpoint)
    }

    fn rename_with_retry(from: &Path, to: &Path) -> std::io::Result<()> {
        // A database handle closing on another thread can hold its files for a
        // moment after the last reference drops.
        let mut last = None;
        for _ in 0..10 {
            match std::fs::rename(from, to) {
                Ok(()) => return Ok(()),
                Err(e) => last = Some(e),
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("rename failed")))
    }

    /// Build checkpoint `id` into a fresh database and swap it in for the
    /// live one, which is kept as `world.fjalldb.pre-restore-<id>`. Elided
    /// files come from commit `sha` when given, else from disk. The database
    /// must be closed.
    fn swap_in(space_root: &Path, id: &str, sha: Option<&str>) -> Result<RestoreSummary, String> {
        let live = space_root.join("world.fjalldb");
        let restoring = space_root.join("world.fjalldb.restoring");
        if restoring.exists() {
            std::fs::remove_dir_all(&restoring).map_err(|e| format!("{}: {e}", restoring.display()))?;
        }
        let mut commit = match sha {
            Some(sha) => Some(CommitFiles::open(space_root, sha)?),
            None => None,
        };
        let summary = eustress_worlddb::restore_checkpoint(
            &checkpoint_file(space_root, id),
            &restoring,
            &mut |_partition, key| {
                let rel = std::str::from_utf8(key).ok()?;
                commit
                    .as_mut()
                    .and_then(|c| c.read(rel))
                    .or_else(|| std::fs::read(space_root.join(rel)).ok())
            },
        );
        drop(commit);
        let summary = match summary {
            Ok(summary) => summary,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&restoring);
                return Err(format!("checkpoint {id}: {e}"));
            }
        };
        let aside = space_root.join(format!("world.fjalldb.pre-restore-{id}"));
        if aside.exists() {
            std::fs::remove_dir_all(&aside).map_err(|e| format!("{}: {e}", aside.display()))?;
        }
        if live.exists() {
            if let Err(e) = rename_with_retry(&live, &aside) {
                let _ = std::fs::remove_dir_all(&restoring);
                return Err(format!(
                    "the database is still in use ({e}); stop hosting or exporting this Space and reopen it to finish the revert"
                ));
            }
        }
        if let Err(e) = rename_with_retry(&restoring, &live) {
            let _ = rename_with_retry(&aside, &live);
            return Err(format!("{}: {e}", live.display()));
        }
        Ok(summary)
    }

    /// Replace the Space's closed database with checkpoint `id`, taking the
    /// files git versions from disk. For tools and tests; a revert goes
    /// through [`request_restore`].
    pub fn restore_db(space_root: &Path, id: &str) -> Result<RestoreSummary, String> {
        valid_id(id)?;
        swap_in(space_root, id, None)
    }

    /// Put the live database back after a swap: the one set aside, or none
    /// when the Space had none.
    fn undo_swap(space_root: &Path, id: &str) {
        let live = space_root.join("world.fjalldb");
        let aside = space_root.join(format!("world.fjalldb.pre-restore-{id}"));
        let _ = std::fs::remove_dir_all(&live);
        if aside.exists() {
            let _ = rename_with_retry(&aside, &live);
        }
    }

    /// Apply a plan, or leave the Space as it was: a failure after the swap
    /// puts the files back from HEAD (the safety snapshot) and the database
    /// back from the one set aside.
    fn apply_plan(space_root: &Path, plan: &RestorePlan) -> Result<Option<RestoreSummary>, String> {
        let summary = match &plan.checkpoint_id {
            Some(id) => Some(swap_in(space_root, id, Some(&plan.git_sha))?),
            None => None,
        };
        if let Err(e) = restore_files(space_root, &plan.git_sha, plan.paths.as_deref()) {
            let back_scope = scope_of(plan.paths.as_deref());
            let mut back = vec!["restore", "--source", "HEAD", "--worktree", "--"];
            back.extend(back_scope.iter().map(String::as_str));
            let files_back = run_git(space_root, &back);
            if let Some(id) = &plan.checkpoint_id {
                undo_swap(space_root, id);
            }
            return Err(match files_back {
                Ok(_) => format!("reverting the files: {e}"),
                Err(b) => format!(
                    "reverting the files: {e}; putting them back from the last commit also failed ({b})"
                ),
            });
        }
        Ok(summary)
    }

    /// Withdraw the plan after a failure and keep its reason.
    fn withdraw(space_root: &Path, reason: &str) -> String {
        let _ = std::fs::remove_file(marker_path(space_root));
        let mut record = toml::map::Map::new();
        record.insert("reason".into(), toml::Value::String(reason.to_string()));
        record.insert("at".into(), toml::Value::String(chrono::Utc::now().to_rfc3339()));
        if let Ok(text) = toml::to_string(&toml::Value::Table(record)) {
            let _ = write_atomic(&failure_path(space_root), text.as_bytes());
        }
        reason.to_string()
    }

    /// Carry out the revert waiting for this Space, if any. Called by the open
    /// path with the Space's database closed, before it opens. Returns the
    /// database restore's summary, `None` when nothing waited or the Space
    /// has no database. A failure leaves the Space as it was and withdraws
    /// the plan ([`last_restore_failure`] keeps why).
    pub fn run_pending_restore(space_root: &Path) -> Result<Option<RestoreSummary>, String> {
        let marker = marker_path(space_root);
        let Ok(text) = std::fs::read_to_string(&marker) else {
            return Ok(None);
        };
        let plan: RestorePlan = match toml::from_str(&text) {
            Ok(plan) => plan,
            Err(e) => return Err(withdraw(space_root, &format!("restore plan: {e}"))),
        };
        let _commit_guard = crate::editor_settings::GIT_COMMIT_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // Every file event from here to the load is this revert's own. A
        // failure ends the window at once: the Space then opens as it was.
        quiet_for(std::time::Duration::from_secs(30));
        let summary = match apply_plan(space_root, &plan) {
            Ok(summary) => summary,
            Err(e) => {
                quiet_for(std::time::Duration::ZERO);
                return Err(withdraw(space_root, &e));
            }
        };
        // The next open reconciles every file changed since a minute before
        // the checkpoint: the files git just wrote, and any the flush wrote
        // that the checkpoint's tree missed while the watcher lagged. The
        // commit's files win, as they do for a closed-engine edit.
        if let Some(id) = &plan.checkpoint_id {
            let since = list_checkpoints(space_root)
                .into_iter()
                .find(|c| &c.id == id)
                .and_then(|c| chrono::DateTime::parse_from_rfc3339(&c.created).ok())
                .map(|t| t.timestamp().saturating_sub(60).max(0) as u64)
                .unwrap_or(0);
            let _ = write_atomic(
                &space_root.join(".eustress").join("last_reconcile"),
                since.to_string().as_bytes(),
            );
        }
        std::fs::remove_file(&marker).map_err(|e| format!("{}: {e}", marker.display()))?;
        quiet_for(std::time::Duration::from_secs(5));
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress_engine_checkpoint_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ids_are_plain_names() {
        assert!(valid_id("snap-2026_09_25").is_ok());
        let long = "x".repeat(65);
        for bad in ["", "../x", "a b", "a/b", long.as_str()] {
            assert!(valid_id(bad).is_err(), "{bad}");
        }
    }

    /// A revert of a Space with a database needs its checkpoint, and a
    /// per-path one waits for the prefix restore; a plan that passes is
    /// written for the next open.
    #[test]
    fn a_restore_request_is_checked_before_it_is_written() {
        let root = temp("request");
        let plan = |paths: Option<Vec<String>>, id: Option<&str>| RestorePlan {
            git_sha: "abc123".to_string(),
            paths,
            checkpoint_id: id.map(str::to_string),
        };
        assert!(request_restore(&root, &RestorePlan { git_sha: "not hex".into(), ..plan(None, None) }).is_err());
        // No database: files alone, whole or per path.
        request_restore(&root, &plan(Some(vec!["Workspace/A".into()]), None)).unwrap();
        assert!(restore_pending(&root));
        std::fs::remove_file(marker_path(&root)).unwrap();

        std::fs::create_dir_all(root.join("world.fjalldb")).unwrap();
        assert!(request_restore(&root, &plan(None, None)).is_err(), "a database needs its checkpoint");
        assert!(request_restore(&root, &plan(None, Some("gone"))).is_err(), "a missing checkpoint");
        std::fs::create_dir_all(snapshots_dir(&root)).unwrap();
        std::fs::write(checkpoint_file(&root, "one"), b"EWCK").unwrap();
        assert!(request_restore(&root, &plan(Some(vec!["Workspace/A".into()]), Some("one"))).is_err());
        request_restore(&root, &plan(None, Some("one"))).unwrap();
        let written: RestorePlan = toml::from_str(&std::fs::read_to_string(marker_path(&root)).unwrap()).unwrap();
        assert_eq!(written, plan(None, Some("one")));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A plan that cannot run is withdrawn with its reason, so saves are
    /// never left refused.
    #[cfg(feature = "world-db")]
    #[test]
    fn a_plan_that_cannot_run_is_withdrawn_with_its_reason() {
        let root = temp("withdraw");
        std::fs::create_dir_all(snapshots_dir(&root)).unwrap();
        std::fs::write(marker_path(&root), "not = [valid").unwrap();
        assert!(run_pending_restore(&root).is_err());
        assert!(!restore_pending(&root), "the plan is withdrawn");
        assert!(last_restore_failure(&root).is_some_and(|r| r.contains("restore plan")));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Reverting one Model puts back only that Model: new work in another
    /// Model survives, and the folder new work left empty goes.
    #[cfg(feature = "world-db")]
    #[test]
    #[ignore = "runs git; run with --ignored"]
    fn a_per_path_revert_touches_only_its_paths() {
        let root = temp("scoped");
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let put = |rel: &str, text: &str| {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        git(&["init", "-q"]);
        put("Workspace/ModelA/part.toml", "a1");
        put("Workspace/ModelB/part.toml", "b1");
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "c1"]);
        let c1 = git(&["rev-parse", "HEAD"]);
        put("Workspace/ModelA/part.toml", "a2");
        put("Workspace/ModelA/added.toml", "new in A");
        put("Workspace/ModelA/Sub/new.toml", "new folder in A");
        put("Workspace/ModelB/added.toml", "new in B");
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "safety"]);

        restore_files(&root, &c1, Some(&["Workspace/ModelA".to_string()])).unwrap();

        let read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
        assert_eq!(read("Workspace/ModelA/part.toml").as_deref(), Some("a1"));
        assert_eq!(read("Workspace/ModelA/added.toml"), None);
        assert!(!root.join("Workspace/ModelA/Sub").exists(), "the emptied folder goes");
        assert_eq!(read("Workspace/ModelB/added.toml").as_deref(), Some("new in B"), "other work survives");
        assert_eq!(read("Workspace/ModelB/part.toml").as_deref(), Some("b1"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_keeps_the_newest() {
        let root = temp("prune");
        let mut index = Index::default();
        for (i, id) in ["a", "b", "c"].iter().enumerate() {
            std::fs::create_dir_all(snapshots_dir(&root)).unwrap();
            std::fs::write(checkpoint_file(&root, id), b"EWCK").unwrap();
            index.checkpoint.push(Checkpoint {
                id: id.to_string(),
                created: format!("2026-09-25T12:0{i}:00Z"),
                seqno: i as u64,
                bytes: 4,
                elided: 0,
                partitions: Vec::new(),
            });
        }
        write_index(&root, &index).unwrap();
        assert_eq!(prune_checkpoints(&root, 2), 1);
        let ids: Vec<String> = list_checkpoints(&root).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["b".to_string(), "c".to_string()]);
        assert!(!checkpoint_file(&root, "a").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
