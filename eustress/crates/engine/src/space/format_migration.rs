//! Moving a Space to the current Space format (`eustress_common::space_format`)
//! when Studio opens it, keeping everything where it was.
//!
//! What changes is decided by the plans in common, which a Player's reader
//! applies in memory to a Space nobody migrated; this module writes them.
//! Format 2 is `wedge_migration::plan`: native wedges and corner wedges turned
//! to fit the Roblox-correct meshes, every pose and face on a turned wedge
//! turned back, Roblox imports left as they are. A baked wedge's core turns
//! exactly when the plan turned the wedge's record (`WedgePlan::turned`); a
//! core may not keep the record's metadata, so it follows the record rather
//! than judging itself.
//!
//! The records come from the UNION of the Space's `.toml` files on disk and
//! the database's tree, disk winning where both hold one (it is the newer when
//! the Space was edited with Studio closed), so a Space whose state lives only
//! in its database moves too. Each rewrite goes to disk where the file lives
//! and to the tree where the key lives, byte for byte the same. Every record
//! is copied first to `.eustress/format_migration/<time>/` (`disk/<key>`,
//! `tree/<key>`, `cores/<id>`) with a `migration.log` of every change, and
//! while records are being written `in_progress` names that folder, so a move
//! that stops partway is put back from its copies on the next open and
//! planned again. One move is one folder, however many steps it takes (format
//! 3 also states every custom mesh's scale kind, `scale_kind::plan`), and a
//! record's copy is the one taken before its first step, so a move stopped
//! after one step is put back whole. The format number is written last, to disk and the tree, and
//! a Space at the current format is never moved again.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use eustress_common::space_format;
use eustress_common::wedge_migration;
use eustress_worlddb::WorldDb;

/// Where the copies and logs go, under the Space's folder.
const MIGRATION_DIR: &str = ".eustress/format_migration";
/// While records are being rewritten: the name of the copy folder.
const IN_PROGRESS: &str = ".eustress/format_migration/in_progress";

/// What a move did.
#[derive(Debug, Default)]
pub struct FormatOutcome {
    /// The format the Space was in.
    pub from: u32,
    /// Native wedges turned.
    pub wedges: usize,
    /// Native corner wedges turned.
    pub corner_wedges: usize,
    /// Poses on a turned wedge, turned back.
    pub children: usize,
    /// Joint frames on a turned wedge, turned back.
    pub joints: usize,
    /// Faces on a turned wedge, remapped.
    pub faces: usize,
    /// Baked wedge cores turned.
    pub cores: usize,
    /// Custom meshes stated a length (imports) and a multiplier (format 3).
    pub scale_lengths: usize,
    pub scale_multipliers: usize,
    /// Files rewritten.
    pub rewritten: usize,
    /// Roblox-imported wedges left as they are.
    pub roblox_left: usize,
    /// Records put back from a move that stopped partway, before planning.
    pub restored: usize,
    /// This move's copies and log.
    pub backup: Option<PathBuf>,
    /// Records left as they are, and why.
    pub problems: Vec<String>,
}

/// Move the Space at `space_root` to the current format. `Ok(None)` when it
/// is there already. `db` is the Space's database, when it has one: each
/// rewrite goes into its tree too, and its baked cores move. On an `Err` the
/// Space keeps its format, loads as it did, and the next open tries again.
pub fn migrate_to_current(space_root: &Path, db: Option<&dyn WorldDb>) -> Result<Option<FormatOutcome>, String> {
    let disk_space_toml = std::fs::read_to_string(space_root.join("space.toml")).ok();
    let tree_space_toml = db
        .and_then(|db| db.get_file("space.toml").ok().flatten())
        .and_then(|bytes| String::from_utf8(bytes).ok());
    let space_toml = disk_space_toml.clone().or_else(|| tree_space_toml.clone());
    let from = space_format::of_space_text(space_toml.as_deref());
    if from >= space_format::CURRENT {
        // A move that wrote the number but stopped before it removed its
        // marker: the number is written last, so the marker is stale.
        let _ = std::fs::remove_file(space_root.join(IN_PROGRESS));
        return Ok(None);
    }

    let mut outcome = FormatOutcome { from, restored: roll_back(space_root, db)?, ..Default::default() };
    let mut moving: Option<Move> = None;
    if space_format::needs_wedge_migration(from) {
        migrate_wedges(space_root, db, &mut outcome, &mut moving)?;
    }
    if space_format::needs_scale_kind_migration(from) {
        migrate_scale_kinds(space_root, db, &mut outcome, &mut moving)?;
    }
    if let Some(moving) = moving {
        write(&moving.backup.join("migration.log"), moving.log.as_bytes())?;
        outcome.backup = Some(moving.backup);
    }

    // The number goes to disk and to the tree, each into its own copy of
    // space.toml (or a new one), so a reader of either sees the Space moved:
    // one that found no number would turn its wedges a second time.
    let disk_text = space_format::with_format(disk_space_toml.as_deref(), space_format::CURRENT);
    write(&space_root.join("space.toml"), disk_text.as_bytes())?;
    if let Some(db) = db {
        let tree_text = space_format::with_format(tree_space_toml.as_deref().or(disk_space_toml.as_deref()), space_format::CURRENT);
        db.put_file("space.toml", tree_text.as_bytes())
            .map_err(|e| format!("space.toml into the database: {e}"))?;
    }
    let _ = std::fs::remove_file(space_root.join(IN_PROGRESS));
    Ok(Some(outcome))
}

/// [`migrate_to_current`], reported to the log. The open hooks call this.
pub fn migrate_logged(space_root: &Path, db: Option<&dyn WorldDb>) {
    match migrate_to_current(space_root, db) {
        Ok(None) => {}
        Ok(Some(o)) => {
            info!(
                target: "eustress_engine::format_migration",
                from = o.from,
                to = space_format::CURRENT,
                rewritten = o.rewritten,
                children = o.children,
                joints = o.joints,
                faces = o.faces,
                roblox_left = o.roblox_left,
                restored = o.restored,
                problems = o.problems.len(),
                backup = ?o.backup,
                space = %space_root.display(),
                "migrated {} wedges, {} corner wedges ({} baked) to the Roblox-correct meshes, and stated the scale of {} custom meshes ({} imports as lengths)",
                o.wedges,
                o.corner_wedges,
                o.cores,
                o.scale_lengths + o.scale_multipliers,
                o.scale_lengths
            );
            for problem in o.problems.iter().take(20) {
                warn!(target: "eustress_engine::format_migration", "left as it is: {problem}");
            }
        }
        Err(e) => warn!(
            target: "eustress_engine::format_migration",
            space = %space_root.display(),
            "the Space keeps its format for this open: {e}"
        ),
    }
}

fn migrate_wedges(
    space_root: &Path,
    db: Option<&dyn WorldDb>,
    outcome: &mut FormatOutcome,
    moving: &mut Option<Move>,
) -> Result<(), String> {
    let (records, on_disk, in_tree) = collect_records(space_root, db, &mut outcome.problems)?;
    let plan = wedge_migration::plan(&records);
    outcome.wedges = plan.wedges;
    outcome.corner_wedges = plan.corner_wedges;
    outcome.children = plan.children;
    outcome.joints = plan.joints;
    outcome.faces = plan.faces;
    outcome.roblox_left = plan.roblox_left;
    outcome.problems.extend(plan.problems.iter().cloned());

    #[cfg(feature = "world-db")]
    let cores = match db {
        Some(db) => plan_cores(db, &plan.turned, &mut outcome.problems)?,
        None => Vec::new(),
    };
    #[cfg(not(feature = "world-db"))]
    let cores: Vec<()> = Vec::new();

    if plan.rewrites.is_empty() && cores.is_empty() {
        return Ok(());
    }

    let moving = Move::begin(space_root, moving)?;
    for rewrite in &plan.rewrites {
        moving.rewrite(space_root, db, &rewrite.key, &rewrite.text, &on_disk, &in_tree)?;
        outcome.rewritten += 1;
        for change in &rewrite.changes {
            moving.log.push_str(&format!("{}: {change}\n", rewrite.key));
        }
    }
    #[cfg(feature = "world-db")]
    if let Some(db) = db {
        outcome.cores = apply_cores(db, cores, &moving.backup, &mut moving.log)?;
    }
    Ok(())
}

/// Format 3: every custom mesh with no `scale_kind` states one
/// (`scale_kind::plan`: a length for a Roblox import, whose meshes are
/// unit-sized, a multiplier for the rest), on disk and in the tree. Planned
/// on the records as the previous step left them.
fn migrate_scale_kinds(
    space_root: &Path,
    db: Option<&dyn WorldDb>,
    outcome: &mut FormatOutcome,
    moving: &mut Option<Move>,
) -> Result<(), String> {
    let (records, on_disk, in_tree) = collect_records(space_root, db, &mut outcome.problems)?;
    let plan = eustress_common::scale_kind::plan(&records);
    outcome.problems.extend(plan.problems.iter().cloned());
    if plan.rewrites.is_empty() {
        return Ok(());
    }
    outcome.scale_lengths = plan.lengths;
    outcome.scale_multipliers = plan.multipliers;
    let moving = Move::begin(space_root, moving)?;
    for (key, text) in &plan.rewrites {
        moving.rewrite(space_root, db, key, text, &on_disk, &in_tree)?;
        outcome.rewritten += 1;
        moving.log.push_str(&format!("{key}: its scale's kind stated\n"));
    }
    Ok(())
}

/// One move's copy folder, log and `in_progress` marker, made by its first
/// step that writes anything and shared by the rest.
struct Move {
    backup: PathBuf,
    log: String,
    /// `disk/<key>` and `tree/<key>` already copied: a record's copy is the
    /// one taken before its first rewrite in this move.
    copied: HashSet<String>,
}

impl Move {
    /// The move under way, begun now if no step has written yet.
    fn begin<'a>(space_root: &Path, moving: &'a mut Option<Move>) -> Result<&'a mut Move, String> {
        if moving.is_none() {
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f").to_string();
            let backup = space_root.join(MIGRATION_DIR).join(&stamp);
            std::fs::create_dir_all(&backup).map_err(|e| format!("{}: {e}", backup.display()))?;
            write(&space_root.join(IN_PROGRESS), stamp.as_bytes())?;
            *moving = Some(Move { backup, log: String::new(), copied: HashSet::new() });
        }
        moving.as_mut().ok_or_else(|| "the move was not begun".to_string())
    }

    /// `text` into `key` on disk where the file lives and in the tree where
    /// the key lives, each place's original copied the first time.
    fn rewrite(
        &mut self,
        space_root: &Path,
        db: Option<&dyn WorldDb>,
        key: &str,
        text: &str,
        on_disk: &BTreeSet<String>,
        in_tree: &BTreeSet<String>,
    ) -> Result<(), String> {
        if on_disk.contains(key) {
            let path = space_root.join(key);
            if self.copied.insert(format!("disk/{key}")) {
                let original = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                write(&self.backup.join("disk").join(key), &original)?;
            }
            write(&path, text.as_bytes())?;
        }
        if let (true, Some(db)) = (in_tree.contains(key), db) {
            if self.copied.insert(format!("tree/{key}")) {
                if let Ok(Some(original)) = db.get_file(key) {
                    write(&self.backup.join("tree").join(key), &original)?;
                }
            }
            db.put_file(key, text.as_bytes()).map_err(|e| format!("{key} into the database: {e}"))?;
        }
        Ok(())
    }
}

/// Every `.toml` record of the Space, from disk and the tree, with the keys
/// each holds. Disk wins where both hold one.
#[allow(clippy::type_complexity)]
fn collect_records(
    space_root: &Path,
    db: Option<&dyn WorldDb>,
    problems: &mut Vec<String>,
) -> Result<(Vec<(String, Vec<u8>)>, BTreeSet<String>, BTreeSet<String>), String> {
    let mut on_disk = BTreeSet::new();
    files_under(space_root, "", false, &mut on_disk)?;
    let mut in_tree = BTreeSet::new();
    if let Some(db) = db {
        let keys = db.iter_tree_keys().map_err(|e| format!("listing the database's tree: {e}"))?;
        in_tree.extend(keys.filter_map(|k| k.ok()).filter(|k| is_record_key(k)));
    }
    let mut records = Vec::new();
    for key in on_disk.union(&in_tree) {
        let bytes = if on_disk.contains(key) {
            std::fs::read(space_root.join(key)).ok()
        } else {
            db.and_then(|db| db.get_file(key).ok().flatten())
        };
        match bytes {
            Some(bytes) => records.push((key.clone(), bytes)),
            None => problems.push(format!("{key}: unreadable")),
        }
    }
    Ok((records, on_disk, in_tree))
}

/// A record the plans read: any `.toml` (instances, services, `space.toml`).
/// The folder structure the plan walks shows in these keys, and a binary
/// file's bytes are never read.
fn is_record_key(key: &str) -> bool {
    key.ends_with(".toml")
}

/// A baked native wedge's core whose record the plan turned.
#[cfg(feature = "world-db")]
struct CoreTurn {
    id: eustress_worlddb::EntityId,
    bytes: Vec<u8>,
    core: eustress_worlddb::ArchInstanceCore,
    shape: eustress_common::classes::PartType,
}

/// The baked cores to turn: a wedge-shaped core turns exactly when the plan
/// turned the record with its uuid. One with no such record is left as it is
/// and reported, rather than judged on what the core kept.
#[cfg(feature = "world-db")]
fn plan_cores(
    db: &dyn WorldDb,
    turned: &[(String, eustress_common::classes::PartType)],
    problems: &mut Vec<String>,
) -> Result<Vec<CoreTurn>, String> {
    let by_uuid: std::collections::HashMap<&str, eustress_common::classes::PartType> =
        turned.iter().map(|(uuid, shape)| (uuid.as_str(), *shape)).collect();
    let cores = db.iter_instance_cores().map_err(|e| format!("reading the baked cores: {e}"))?;
    let mut planned = Vec::new();
    for (id, bytes) in cores {
        let Ok(core) = eustress_worlddb::decode_instance_core(&bytes) else { continue };
        if eustress_common::part_meshes::native_wedge_shape(&core.class_name, Some(&core.mesh)).is_none() {
            continue;
        }
        let synthetic = super::bake_cores::synthetic_rel(&core.class_name, id.0);
        let uuid = db
            .path_to_uuid(&synthetic)
            .ok()
            .flatten()
            .map(|u| wedge_migration::normalise_uuid(&u.iter().map(|b| format!("{b:02x}")).collect::<String>()));
        match uuid.as_deref().and_then(|u| by_uuid.get(u)) {
            Some(shape) => planned.push(CoreTurn { id, bytes, core, shape: *shape }),
            None => problems.push(format!(
                "baked core {:016x}: a wedge with no turned record to follow; left as it is",
                id.0
            )),
        }
    }
    Ok(planned)
}

/// Turn each planned core, and its uuid-keyed copy.
#[cfg(feature = "world-db")]
fn apply_cores(db: &dyn WorldDb, cores: Vec<CoreTurn>, backup: &Path, log: &mut String) -> Result<usize, String> {
    let mut turned = 0;
    for CoreTurn { id, bytes, mut core, shape } in cores {
        write(&backup.join("cores").join(format!("{:016x}", id.0)), &bytes)?;
        let (rotation, size) = eustress_common::part_meshes::migrate_native_wedge(
            shape,
            Quat::from_array(core.r),
            Vec3::from_array(core.s),
        );
        core.r = rotation.to_array();
        core.s = size.to_array();
        let encoded = eustress_worlddb::encode_instance_core(&core).map_err(|e| format!("core {:016x}: {e}", id.0))?;
        db.put_instance_core(id, (core.t[0], core.t[1], core.t[2]), &encoded)
            .map_err(|e| format!("core {:016x}: {e}", id.0))?;
        let synthetic = super::bake_cores::synthetic_rel(&core.class_name, id.0);
        if let Ok(Some(uuid)) = db.path_to_uuid(&synthetic) {
            db.put_entity_core_by_uuid(&uuid, &encoded)
                .map_err(|e| format!("core {:016x}'s uuid copy: {e}", id.0))?;
        }
        log.push_str(&format!("core {:016x}: {shape:?} turned\n", id.0));
        turned += 1;
    }
    Ok(turned)
}

// ── Copies, roll-back, files ───────────────────────────────────────────────

/// Put back every record a move that stopped partway had rewritten, from its
/// copies, and forget that move. How many were put back.
fn roll_back(space_root: &Path, db: Option<&dyn WorldDb>) -> Result<usize, String> {
    let marker = space_root.join(IN_PROGRESS);
    let Ok(stamp) = std::fs::read_to_string(&marker) else { return Ok(0) };
    let backup = space_root.join(MIGRATION_DIR).join(stamp.trim());
    let mut restored = 0;
    let mut copies = BTreeSet::new();
    files_under(&backup.join("disk"), "", true, &mut copies)?;
    for key in &copies {
        let bytes = std::fs::read(backup.join("disk").join(key)).map_err(|e| format!("{key}: {e}"))?;
        write(&space_root.join(key), &bytes)?;
        restored += 1;
    }
    if let Some(db) = db {
        let mut copies = BTreeSet::new();
        files_under(&backup.join("tree"), "", true, &mut copies)?;
        for key in &copies {
            let bytes = std::fs::read(backup.join("tree").join(key)).map_err(|e| format!("{key}: {e}"))?;
            db.put_file(key, &bytes).map_err(|e| format!("{key} into the database: {e}"))?;
            restored += 1;
        }
        #[cfg(feature = "world-db")]
        {
            restored += restore_cores(db, &backup.join("cores"))?;
        }
    }
    std::fs::remove_file(&marker).map_err(|e| format!("{}: {e}", marker.display()))?;
    Ok(restored)
}

#[cfg(feature = "world-db")]
fn restore_cores(db: &dyn WorldDb, dir: &Path) -> Result<usize, String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(0) };
    let mut restored = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(id) = u64::from_str_radix(&name, 16) else { continue };
        let bytes = std::fs::read(entry.path()).map_err(|e| format!("core {name}: {e}"))?;
        let core = eustress_worlddb::decode_instance_core(&bytes).map_err(|e| format!("core {name}: {e}"))?;
        db.put_instance_core(eustress_worlddb::EntityId(id), (core.t[0], core.t[1], core.t[2]), &bytes)
            .map_err(|e| format!("core {name}: {e}"))?;
        let synthetic = super::bake_cores::synthetic_rel(&core.class_name, id);
        if let Ok(Some(uuid)) = db.path_to_uuid(&synthetic) {
            let _ = db.put_entity_core_by_uuid(&uuid, &bytes);
        }
        restored += 1;
    }
    Ok(restored)
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Every record file under `dir` (every file, with `all`), keyed relative to
/// it with `/`, leaving out hidden folders and the database's folders.
fn files_under(dir: &Path, prefix: &str, all: bool, out: &mut BTreeSet<String>) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(()) };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let key = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            if name.starts_with('.') || name.starts_with("world.fjalldb") {
                continue;
            }
            files_under(&entry.path(), &key, all, out)?;
        } else if all || is_record_key(&key) {
            out.insert(key);
        }
    }
    Ok(())
}

/// The format a Space names on disk when it is newer than this build can read
/// (`space_format::CURRENT`), or `None` when this build opens it. Read from the
/// Space's own `space.toml`: nothing is written and no database is opened.
pub fn newer_than_build(space_root: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(space_root.join("space.toml")).ok()?;
    let format = space_format::of_space_text(Some(&text));
    (format > space_format::CURRENT).then_some(format)
}

/// What a person is told about a Space this build will not open.
pub fn newer_space_message(format: u32) -> String {
    format!(
        "This Space was made for Eustress format {format}. This build opens format {} and older, so the Space was not opened and nothing in it was changed. Update Eustress to open it.",
        space_format::CURRENT
    )
}

/// A refused launch's message, waiting for the first frame. Set by
/// [`launch_space_or_scratch`] and shown once by [`announce_refused_launch`].
static REFUSED_LAUNCH: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// The Space the editor opens at launch: `requested`, or, when it is a newer
/// Space this build will not open, an empty scratch Space in the temp folder.
/// Nothing reads or writes the newer Space, and the person is told why.
pub fn launch_space_or_scratch(requested: PathBuf) -> PathBuf {
    let Some(format) = newer_than_build(&requested) else {
        return requested;
    };
    let parent = std::env::temp_dir();
    let name = "Eustress scratch Space";
    let _ = super::space_ops::scaffold_new_space(&parent, name, "Eustress");
    if let Ok(mut slot) = REFUSED_LAUNCH.lock() {
        *slot = Some(newer_space_message(format));
    }
    parent.join(name)
}

/// Shows a refused launch's message once, in the notification list.
pub fn announce_refused_launch(notifications: Option<ResMut<crate::notifications::NotificationManager>>) {
    let message = REFUSED_LAUNCH.lock().ok().and_then(|mut slot| slot.take());
    if let (Some(message), Some(mut notifications)) = (message, notifications) {
        notifications.warning(message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("eustress_format_migration_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn put(root: &Path, key: &str, text: &str) {
        write(&root.join(key), text.as_bytes()).unwrap();
    }

    /// The plan's rewrite reaches the file, the number is written with the
    /// rest of space.toml kept, the copy and log are left behind, and a second
    /// open does nothing. A native wedge is a Part drawn by `parts/wedge.glb`,
    /// as Studio writes one.
    #[test]
    fn a_format_one_space_moves_once() {
        let root = space("once");
        put(&root, "space.toml", "[space]\nname = \"Garage\"\n");
        put(
            &root,
            "Workspace/Roof/_instance.toml",
            "[metadata]\nclass_name = \"Part\"\nuuid = \"aaaa-0001\"\n\n[asset]\nmesh = \"parts/wedge.glb\"\n\n[transform]\nposition = [0.0, 1.0, 0.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [2.0, 1.0, 4.0]\n",
        );
        let before = std::fs::read_to_string(root.join("Workspace/Roof/_instance.toml")).unwrap();

        let outcome = migrate_to_current(&root, None).unwrap().expect("a format-1 Space moves");
        assert_eq!(outcome.from, 1);
        assert_eq!(outcome.wedges, 1);
        assert_eq!(outcome.rewritten, 1);
        let after = std::fs::read_to_string(root.join("Workspace/Roof/_instance.toml")).unwrap();
        assert_ne!(after, before, "the wedge's file was rewritten");
        let doc: toml::Value = after.parse().unwrap();
        let q: Vec<f64> = doc["transform"]["rotation"].as_array().unwrap().iter().map(|v| v.as_float().unwrap()).collect();
        assert!((q[1].abs() - 1.0).abs() < 1e-5 && q[0].abs() + q[2].abs() + q[3].abs() < 1e-5, "a half turn about up: {q:?}");
        let backup = outcome.backup.expect("a copy folder");
        assert_eq!(std::fs::read_to_string(backup.join("disk/Workspace/Roof/_instance.toml")).unwrap(), before);
        assert!(backup.join("migration.log").exists());
        assert!(!root.join(IN_PROGRESS).exists());

        let space_toml: toml::Value = std::fs::read_to_string(root.join("space.toml")).unwrap().parse().unwrap();
        assert_eq!(space_format::of_space(Some(&space_toml)), space_format::CURRENT);
        assert_eq!(space_toml["space"]["name"].as_str(), Some("Garage"));

        assert!(migrate_to_current(&root, None).unwrap().is_none(), "a second open does nothing");
        assert_eq!(std::fs::read_to_string(root.join("Workspace/Roof/_instance.toml")).unwrap(), after);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A Space with nothing to turn only gets its number, and no copy folder.
    #[test]
    fn a_space_without_wedges_only_gets_its_number() {
        let root = space("none");
        put(&root, "Workspace/Box/_instance.toml", "[metadata]\nclass_name = \"Part\"\n");
        let outcome = migrate_to_current(&root, None).unwrap().unwrap();
        assert_eq!((outcome.rewritten, outcome.wedges), (0, 0));
        assert!(outcome.backup.is_none());
        assert!(!root.join(MIGRATION_DIR).exists());
        assert_eq!(space_format::of_space_text(std::fs::read_to_string(root.join("space.toml")).ok().as_deref()), space_format::CURRENT);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A move that stopped partway is put back from its copies before the
    /// next one plans.
    #[test]
    fn a_move_that_stopped_partway_is_put_back() {
        let root = space("rollback");
        put(&root, "Workspace/A/_instance.toml", "[metadata]\nclass_name = \"Part\"\n");
        put(&root, &format!("{MIGRATION_DIR}/stamp/disk/Workspace/A/_instance.toml"), "original = true\n");
        put(&root, IN_PROGRESS, "stamp");
        assert_eq!(roll_back(&root, None).unwrap(), 1);
        assert_eq!(std::fs::read_to_string(root.join("Workspace/A/_instance.toml")).unwrap(), "original = true\n");
        assert!(!root.join(IN_PROGRESS).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A format-1 Space takes both moves in one pass and one copy folder: the
    /// wedge turns, an import states a length and a native mesh a multiplier,
    /// and a record both moves rewrote is copied as it was before either.
    #[test]
    fn both_moves_share_one_copy_folder() {
        let root = space("both");
        put(&root, "space.toml", "[space]\nname = \"Garage\"\n");
        put(
            &root,
            "Workspace/Roof/_instance.toml",
            "[metadata]\nclass_name = \"Part\"\nuuid = \"aaaa-0001\"\n\n[asset]\nmesh = \"parts/wedge.glb\"\n\n[transform]\nposition = [0.0, 1.0, 0.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [2.0, 1.0, 4.0]\n",
        );
        let vent = "[metadata]\nclass_name = \"Part\"\n\n[asset]\nmesh = \"meshes/vent.glb\"\n\n[transform]\nposition = [0.5, 0.25, 0.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [1.0, 1.0, 1.0]\n";
        put(&root, "Workspace/Roof/Vent/_instance.toml", vent);
        put(
            &root,
            "Workspace/HotDog/_instance.toml",
            "[metadata]\nclass_name = \"Part\"\nunit = \"stud\"\nroblox_color_srgb = [1.0, 0.5, 0.2]\n\n[asset]\nmesh = \"assets/meshes/rbx-1.glb\"\n\n[transform]\nscale = [1.0, 0.5, 0.5]\n",
        );

        let outcome = migrate_to_current(&root, None).unwrap().expect("a format-1 Space moves");
        assert_eq!(outcome.wedges, 1);
        assert_eq!((outcome.scale_lengths, outcome.scale_multipliers), (1, 1));
        let backup = outcome.backup.expect("a copy folder");
        let folders: Vec<_> = std::fs::read_dir(root.join(MIGRATION_DIR)).unwrap().flatten().filter(|e| e.path().is_dir()).collect();
        assert_eq!(folders.len(), 1, "one move, one folder");
        assert_eq!(std::fs::read_to_string(backup.join("disk/Workspace/Roof/Vent/_instance.toml")).unwrap(), vent, "the copy is the original");
        let doc = |key: &str| -> toml::Value { std::fs::read_to_string(root.join(key)).unwrap().parse().unwrap() };
        assert_eq!(doc("Workspace/Roof/Vent/_instance.toml")["asset"]["scale_kind"].as_str(), Some("multiplier"));
        assert_eq!(doc("Workspace/HotDog/_instance.toml")["asset"]["scale_kind"].as_str(), Some("length"));
        assert_eq!(space_format::of_space(Some(&doc("space.toml"))), space_format::CURRENT);
        assert!(!root.join(IN_PROGRESS).exists());

        // Put back from that one folder, every record is as it was before the move.
        put(&root, IN_PROGRESS, &backup.file_name().unwrap().to_string_lossy());
        roll_back(&root, None).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("Workspace/Roof/Vent/_instance.toml")).unwrap(), vent);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A format-2 Space only states its custom meshes' scale kinds.
    #[test]
    fn a_format_two_space_only_states_scale_kinds() {
        let root = space("two");
        put(&root, "space.toml", "[space]\nformat = 2\n");
        put(&root, "Workspace/Crate/_instance.toml", "[metadata]\nclass_name = \"Part\"\n\n[asset]\nmesh = \"meshes/crate.glb\"\n");
        let outcome = migrate_to_current(&root, None).unwrap().unwrap();
        assert_eq!((outcome.wedges, outcome.scale_multipliers, outcome.rewritten), (0, 1, 1));
        let text = std::fs::read_to_string(root.join("Workspace/Crate/_instance.toml")).unwrap();
        assert!(text.contains("scale_kind = \"multiplier\""), "{text}");
        assert!(migrate_to_current(&root, None).unwrap().is_none(), "a second open does nothing");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_toml_records_are_read() {
        assert!(is_record_key("Workspace/A/_instance.toml"));
        assert!(is_record_key("Workspace/_service.toml"));
        assert!(is_record_key("space.toml"));
        assert!(!is_record_key("Workspace/Car/Body.glb"));
    }
}

#[cfg(test)]
mod newer_space_tests {
    use super::*;

    fn space(tag: &str, format: u32) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("eustress_newer_{tag}_{}_{nanos}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("space.toml"), format!("[space]\nname = \"Garage\"\nformat = {format}\n")).unwrap();
        root
    }

    /// A Space a newer build made is refused; one this build reads opens. Without
    /// the check, the newer Space would be opened and written back by rules this
    /// build does not know.
    #[test]
    fn a_newer_space_is_refused_and_an_older_one_opens() {
        let newer = space("newer", space_format::CURRENT + 1);
        let current = space("current", space_format::CURRENT);
        let older = space("older", 1);
        let missing = std::env::temp_dir().join("eustress_newer_no_such_space");
        assert_eq!(newer_than_build(&newer), Some(space_format::CURRENT + 1));
        assert_eq!(newer_than_build(&current), None);
        assert_eq!(newer_than_build(&older), None);
        assert_eq!(newer_than_build(&missing), None, "no space.toml, nothing to refuse");
        assert!(newer_space_message(4).contains("format 4"));
        for root in [newer, current, older] {
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// The launch keeps a Space it can open, and turns a newer one into a
    /// scratch Space, with the reason kept for the first frame.
    #[test]
    fn a_newer_launch_space_becomes_a_scratch_space() {
        let older = space("launch_older", 1);
        assert_eq!(launch_space_or_scratch(older.clone()), older);
        let newer = space("launch_newer", space_format::CURRENT + 1);
        let opened = launch_space_or_scratch(newer.clone());
        assert_ne!(opened, newer, "the newer Space is not opened");
        assert!(opened.is_dir(), "the scratch Space exists");
        let message = REFUSED_LAUNCH.lock().unwrap().clone().expect("the refusal is kept");
        assert!(message.contains(&format!("format {}", space_format::CURRENT + 1)), "{message}");
        let _ = std::fs::remove_dir_all(&newer);
        let _ = std::fs::remove_dir_all(&older);
    }
}
