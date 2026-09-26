//! Moving a Space written under the legacy pose rule to `ParentPose` when it
//! opens, keeping every instance where it was.
//!
//! `eustress_common::pose_migration` plans which files change and to what;
//! here they are written. Every file is copied to
//! `.eustress/transform_migration/<time>/` before it is rewritten, and a
//! `migration.log` there lists each one with its rule and its numbers before
//! and after. A Space with a database takes each file into its tree as disk
//! does, before anything reads the tree. While files are being written,
//! `in_progress` names the copy folder, so a move that stops partway is put
//! back from its copies on the next open and planned again. `space.toml`
//! names the rule last, and a Space that names it is never moved again.
//!
//! Opening a Space moves it only with `EUSTRESS_MIGRATE_POSE=1` set
//! ([`migration_enabled`]); without it a legacy Space opens under the legacy
//! rule and nothing on disk changes.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bevy::prelude::*;
use eustress_common::datamodel::record::TransformRule;
use eustress_common::pose_migration::{plan, rewrite_text, with_parent_pose_rule, Placement, Rewrite};
use eustress_common::tree_read::space_records;
use eustress_worlddb::WorldDb;

/// Where the copies and logs go, under the Space's folder.
const MIGRATION_DIR: &str = ".eustress/transform_migration";
/// While files are being rewritten: the name of the copy folder.
const IN_PROGRESS: &str = ".eustress/transform_migration/in_progress";
/// A file the importer wrote is unsaved since the import when it is newer
/// than the import report by no more than this.
const IMPORT_WINDOW: Duration = Duration::from_secs(60);

/// What a move did.
#[derive(Debug, Default)]
pub struct MigrationOutcome {
    /// Files rewritten.
    pub rewritten: usize,
    /// Of those, placed at their Roblox world pose.
    pub roblox_world: usize,
    /// Of those, drawn sheared and given the nearest pose and size.
    pub sheared: usize,
    /// Files put back from a move that stopped partway, before planning.
    pub restored: usize,
    /// This move's copies and log.
    pub backup: Option<PathBuf>,
    /// Files left as they are, and why.
    pub problems: Vec<String>,
}

/// Move the Space at `space_root` to the `ParentPose` rule. `Ok(None)` when
/// its `space.toml` names the rule already, or it has no files on disk to
/// move. `db` is the Space's database, when it has one: each file goes into
/// its tree too. On an `Err` the Space stays on the legacy rule, loads as it
/// did, and the next open tries again.
pub fn migrate_to_parent_pose(space_root: &Path, db: Option<&dyn WorldDb>) -> Result<Option<MigrationOutcome>, String> {
    let space_toml_path = space_root.join("space.toml");
    let space_toml = std::fs::read_to_string(&space_toml_path).ok();
    let named = space_toml.as_deref().and_then(|text| text.parse::<toml::Value>().ok());
    if TransformRule::of_space(named.as_ref()) == TransformRule::ParentPose {
        // A move that named the rule but stopped before it removed its
        // marker: the key is written last, so the marker is stale. Its copies
        // stay.
        let _ = std::fs::remove_file(space_root.join(IN_PROGRESS));
        return Ok(None);
    }
    if !space_root.join("Workspace").is_dir() {
        return Ok(None);
    }

    let mut outcome = MigrationOutcome { restored: roll_back(space_root, db)?, ..Default::default() };
    let (records, problems) = space_records(space_root)?;
    outcome.problems = problems;
    let report_time = modified(&space_root.join(".eustress/import_report.json"));
    let unsaved = |key: &str, doc: &toml::Value| unsaved_import(space_root, report_time, key, doc);
    let plan = plan(&records, &unsaved)?;
    outcome.problems.extend(plan.problems.iter().cloned());

    if !plan.rewrites.is_empty() {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f").to_string();
        let backup = space_root.join(MIGRATION_DIR).join(&stamp);
        std::fs::create_dir_all(&backup).map_err(|e| format!("{}: {e}", backup.display()))?;
        write(&space_root.join(IN_PROGRESS), stamp.as_bytes())?;
        let mut log = String::new();
        for rewrite in &plan.rewrites {
            let path = space_root.join(&rewrite.key);
            let original = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            write(&backup.join(&rewrite.key), original.as_bytes())?;
            let text = rewrite_text(&original, rewrite)?;
            write(&path, text.as_bytes())?;
            if let Some(db) = db {
                put_tree(db, &rewrite.key, text.as_bytes())?;
            }
            outcome.rewritten += 1;
            outcome.roblox_world += usize::from(rewrite.placement == Placement::RobloxWorld);
            outcome.sheared += usize::from(rewrite.sheared);
            log.push_str(&log_line(rewrite));
        }
        write(&backup.join("migration.log"), log.as_bytes())?;
        outcome.backup = Some(backup);
    }

    let text = with_parent_pose_rule(space_toml.as_deref());
    write(&space_toml_path, text.as_bytes())?;
    if let Some(db) = db {
        put_tree(db, "space.toml", text.as_bytes())?;
    }
    let _ = std::fs::remove_file(space_root.join(IN_PROGRESS));
    Ok(Some(outcome))
}

/// Whether opening a Space moves it to the `ParentPose` rule:
/// `EUSTRESS_MIGRATE_POSE=1`.
pub fn migration_enabled() -> bool {
    std::env::var("EUSTRESS_MIGRATE_POSE").is_ok_and(|v| v.trim() == "1")
}

/// [`migrate_to_parent_pose`] when [`migration_enabled`], reported to the
/// log. The open hooks call this.
pub fn migrate_logged(space_root: &Path, db: Option<&dyn WorldDb>) {
    if !migration_enabled() {
        return;
    }
    match migrate_to_parent_pose(space_root, db) {
        Ok(None) => {}
        Ok(Some(o)) => info!(
            target: "eustress_engine::rule_migration",
            rewritten = o.rewritten,
            roblox_world = o.roblox_world,
            sheared = o.sheared,
            restored = o.restored,
            problems = o.problems.len(),
            backup = ?o.backup,
            space = %space_root.display(),
            "the Space now composes each child onto its parent's pose (transform_rule = \"parent_pose\")"
        ),
        Err(e) => warn!(
            target: "eustress_engine::rule_migration",
            space = %space_root.display(),
            "the Space stays on the legacy pose rule for this open: {e}"
        ),
    }
}

/// Put back every file a move that stopped partway had rewritten, from its
/// copies, and forget that move. How many were put back.
fn roll_back(space_root: &Path, db: Option<&dyn WorldDb>) -> Result<usize, String> {
    let marker = space_root.join(IN_PROGRESS);
    let Ok(stamp) = std::fs::read_to_string(&marker) else { return Ok(0) };
    let stamp = stamp.trim();
    if stamp.is_empty() || !stamp.chars().all(|c| c.is_ascii_digit() || c == '-') {
        return Err(format!("{} does not name a move", marker.display()));
    }
    let backup = space_root.join(MIGRATION_DIR).join(stamp);
    let mut copies = Vec::new();
    files_under(&backup, "", &mut copies)?;
    let mut restored = 0;
    for (rel, copy) in copies {
        if rel == "migration.log" {
            continue;
        }
        let bytes = std::fs::read(&copy).map_err(|e| format!("{}: {e}", copy.display()))?;
        write(&space_root.join(&rel), &bytes)?;
        if let Some(db) = db {
            put_tree(db, &rel, &bytes)?;
        }
        restored += 1;
    }
    std::fs::remove_file(&marker).map_err(|e| format!("{}: {e}", marker.display()))?;
    Ok(restored)
}

/// Whether the importer wrote this record and nothing has saved it since:
/// it carries the importer's Roblox colour, no `last_modified` (every signed
/// save stamps one), and is no newer than the import report (a save through
/// an unsigned path leaves no stamp, but a newer file).
fn unsaved_import(space_root: &Path, report_time: Option<SystemTime>, key: &str, doc: &toml::Value) -> bool {
    let Some(report_time) = report_time else { return false };
    let Some(meta) = doc.get("metadata").and_then(|m| m.as_table()) else { return false };
    let imported = meta.contains_key("roblox_color_srgb") || meta.contains_key("roblox_brick_color");
    imported
        && !meta.contains_key("last_modified")
        && modified(&space_root.join(key)).is_some_and(|t| t <= report_time + IMPORT_WINDOW)
}

/// One file into the database tree, as the reconcile puts it: its bytes, and
/// its `#bin` twin dropped so the tree is read fresh.
fn put_tree(db: &dyn WorldDb, rel: &str, bytes: &[u8]) -> Result<(), String> {
    db.put_file(rel, bytes).map_err(|e| format!("{rel} into the database: {e}"))?;
    let _ = db.delete_file(&format!("{rel}#bin"));
    Ok(())
}

fn log_line(r: &Rewrite) -> String {
    let (p, q, s) = r.before;
    let placement = match r.placement {
        Placement::RobloxWorld => "Roblox world pose",
        Placement::AsDrawn => "as drawn",
    };
    let scale = match r.scale {
        Some(new) => format!("  scale {s:?} -> {new:?}"),
        None => String::new(),
    };
    let sheared = if r.sheared { "  (drawn sheared: nearest pose and size)" } else { "" };
    format!(
        "{}  [{placement}]  position {p:?} -> {:?}  rotation {q:?} -> {:?}{scale}{sheared}\n",
        r.key, r.position, r.rotation
    )
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Every file under `dir`, as (its path relative to `dir` with `/`, its path).
fn files_under(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, &rel, out)?;
        } else {
            out.push((rel, path));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_space(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("eustress_rule_migration_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let put = |rel: &str, text: &str| write(&root.join(rel), text.as_bytes()).unwrap();
        put("space.toml", "[space]\nname = \"Garage\"\n");
        put("Workspace/_service.toml", "[service]\nclass_name = \"Workspace\"\n");
        put(
            "Workspace/Base/_instance.toml",
            "[metadata]\nclass_name = \"Part\"\n\n[transform]\nposition = [0.0, 1.0, 0.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [2.0, 1.0, 2.0]\n",
        );
        put(
            "Workspace/Base/Flag/_instance.toml",
            "[metadata]\nclass_name = \"Part\"\n\n[transform]\nposition = [0.25, 1.5, 0.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [0.2, 2.0, 0.2]\n",
        );
        root
    }

    const FLAG: &str = "Workspace/Base/Flag/_instance.toml";

    /// Without the switch, opening a legacy Space changes nothing on disk.
    #[test]
    fn opening_moves_nothing_unless_switched_on() {
        if migration_enabled() {
            return; // the switch is on in this shell
        }
        let root = temp_space("off");
        let before = std::fs::read_to_string(root.join(FLAG)).unwrap();
        let space_before = std::fs::read_to_string(root.join("space.toml")).unwrap();
        migrate_logged(&root, None);
        assert_eq!(std::fs::read_to_string(root.join(FLAG)).unwrap(), before);
        assert_eq!(std::fs::read_to_string(root.join("space.toml")).unwrap(), space_before);
        assert!(!root.join(MIGRATION_DIR).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A Space moves once: the rewritten file is copied and logged first,
    /// space.toml names the rule, and the next open does nothing.
    #[test]
    fn a_space_moves_once_and_keeps_its_copies() {
        let root = temp_space("once");
        let original = std::fs::read_to_string(root.join(FLAG)).unwrap();
        let outcome = migrate_to_parent_pose(&root, None).unwrap().expect("moved");
        assert_eq!(outcome.rewritten, 1, "{outcome:?}");
        let backup = outcome.backup.expect("copies");
        assert_eq!(std::fs::read_to_string(backup.join(FLAG)).unwrap(), original);
        let log = std::fs::read_to_string(backup.join("migration.log")).unwrap();
        assert!(log.contains(FLAG) && log.contains("as drawn"), "{log}");
        // Drawn 0.4 by 2 by 0.4 under a base 2 by 1 by 2: that size now.
        let doc: toml::Value = std::fs::read_to_string(root.join(FLAG)).unwrap().parse().unwrap();
        let scale: Vec<f64> = doc["transform"]["scale"].as_array().unwrap().iter().map(|v| v.as_float().unwrap()).collect();
        assert!((scale[0] - 0.4).abs() < 1e-5 && (scale[1] - 2.0).abs() < 1e-5 && (scale[2] - 0.4).abs() < 1e-5, "{scale:?}");
        let space: toml::Value = std::fs::read_to_string(root.join("space.toml")).unwrap().parse().unwrap();
        assert_eq!(TransformRule::of_space(Some(&space)), TransformRule::ParentPose);
        assert_eq!(space["space"]["name"].as_str(), Some("Garage"));
        assert!(!root.join(IN_PROGRESS).exists());
        let moved = std::fs::read_to_string(root.join(FLAG)).unwrap();
        // A marker left by a move that stopped after naming the rule.
        write(&root.join(IN_PROGRESS), b"20260924-000000-000").unwrap();
        assert!(migrate_to_parent_pose(&root, None).unwrap().is_none(), "a second open does nothing");
        assert_eq!(std::fs::read_to_string(root.join(FLAG)).unwrap(), moved);
        assert!(!root.join(IN_PROGRESS).exists(), "a stale marker is forgotten");
        assert!(backup.join(FLAG).exists(), "the copies stay");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A move that stopped partway is put back from its copies and done
    /// again from the start, landing where a clean move lands.
    #[test]
    fn a_move_that_stopped_partway_is_put_back_and_redone() {
        let clean = temp_space("clean");
        migrate_to_parent_pose(&clean, None).unwrap().expect("moved");
        let expected = std::fs::read_to_string(clean.join(FLAG)).unwrap();

        let root = temp_space("stopped");
        let original = std::fs::read_to_string(root.join(FLAG)).unwrap();
        let stamp = "20260924-210000-000";
        write(&root.join(MIGRATION_DIR).join(stamp).join(FLAG), original.as_bytes()).unwrap();
        write(&root.join(IN_PROGRESS), stamp.as_bytes()).unwrap();
        // Half written: this file already holds numbers of the new rule.
        write(&root.join(FLAG), expected.as_bytes()).unwrap();

        let outcome = migrate_to_parent_pose(&root, None).unwrap().expect("moved");
        assert_eq!(outcome.restored, 1);
        assert_eq!(outcome.rewritten, 1);
        assert_eq!(std::fs::read_to_string(root.join(FLAG)).unwrap(), expected, "never converted twice");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&clean);
    }

    /// A real Space, copied: every part lands where it was drawn, except the
    /// untouched imports, which land at their Roblox world pose, and sheared
    /// children. The source is only read: every file's time is the same after.
    /// Set `EUSTRESS_MIGRATION_SAMPLE` to a Space on the legacy rule; the run
    /// prints the parts checked, the largest deviation and its time.
    #[test]
    #[ignore = "real data: set EUSTRESS_MIGRATION_SAMPLE to a Space folder"]
    fn a_real_space_moves_without_moving_anything() {
        use eustress_common::datamodel::DmValue;
        use eustress_common::tree_read::{read_space_records, SceneTree};
        let Some(sample) = std::env::var_os("EUSTRESS_MIGRATION_SAMPLE").map(PathBuf::from) else { return };
        let started = std::time::Instant::now();
        let root = std::env::temp_dir().join(format!("eustress_rule_migration_real_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // The files the move reads, with their times (the import report's
        // and each file's decide what is untouched), and the source's times,
        // to show it is only read.
        let mut files = Vec::new();
        files_under(&sample, "", &mut files).unwrap();
        let mut source_times = Vec::new();
        for (rel, path) in files {
            let keep = rel == ".eustress/import_report.json"
                || (!rel.starts_with('.') && !rel.starts_with("world.fjalldb") && rel.ends_with(".toml"));
            if !keep {
                continue;
            }
            let time = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            let dest = root.join(&rel);
            write(&dest, &std::fs::read(&path).unwrap()).unwrap();
            if let Some(t) = time {
                let _ = std::fs::File::options().write(true).open(&dest).and_then(|f| f.set_modified(t));
            }
            source_times.push((path, time));
        }

        let (before_records, _) = space_records(&root).unwrap();
        let before = read_space_records(&before_records);
        let outcome = migrate_to_parent_pose(&root, None).unwrap().expect("a legacy Space");
        let (after_records, _) = space_records(&root).unwrap();
        let after = read_space_records(&after_records);
        let log = outcome
            .backup
            .as_ref()
            .map(|b| std::fs::read_to_string(b.join("migration.log")).unwrap())
            .unwrap_or_default();
        let exempt: std::collections::HashSet<&str> = log
            .lines()
            .filter(|l| l.contains("[Roblox world pose]") || l.contains("drawn sheared"))
            .filter_map(|l| l.split("  ").next())
            .collect();
        let cframe = |tree: &SceneTree, id| match tree.dm.get_prop(id, "CFrame") {
            Some(DmValue::CFrame(cf)) => {
                let m = cf.rotation_matrix();
                Some([
                    cf.position.x, cf.position.y, cf.position.z,
                    m[0][0], m[0][1], m[0][2], m[1][0], m[1][1], m[1][2], m[2][0], m[2][1], m[2][2],
                ])
            }
            _ => None,
        };
        let after_ids: std::collections::HashMap<&str, _> = after.keys.iter().map(|(k, id)| (k.as_str(), *id)).collect();
        let (mut compared, mut missing) = (0usize, Vec::new());
        let (mut worst_position, mut worst_rotation) = (0.0f64, 0.0f64);
        let mut worst_key = String::new();
        for (key, id) in &before.keys {
            // Parts only: an Attachment moves by design (the legacy rule drew
            // it at its part's origin).
            if exempt.contains(key.as_str()) || before.dm.get_prop(*id, "Size").is_none() {
                continue;
            }
            let Some(a) = cframe(&before, *id) else { continue };
            let Some(b) = after_ids.get(key.as_str()).and_then(|other| cframe(&after, *other)) else {
                missing.push(key.clone());
                continue;
            };
            let position = (0..3).map(|i| (a[i] - b[i]).abs()).fold(0.0, f64::max);
            let rotation = (3..12).map(|i| (a[i] - b[i]).abs()).fold(0.0, f64::max);
            if position.max(rotation) > worst_position.max(worst_rotation) {
                worst_key = key.clone();
            }
            worst_position = worst_position.max(position);
            worst_rotation = worst_rotation.max(rotation);
            compared += 1;
        }
        let _ = std::fs::remove_dir_all(&root);
        let changed: Vec<String> = source_times
            .iter()
            .filter(|(path, time)| std::fs::metadata(path).and_then(|m| m.modified()).ok() != *time)
            .map(|(path, _)| path.display().to_string())
            .collect();
        eprintln!(
            "{}: {compared} parts checked; largest deviation {worst_position:.2e} m in position and {worst_rotation:.2e} \
             in rotation (at {worst_key}); {} files rewritten ({} at their Roblox pose, {} sheared); {} source files, \
             {} with a changed time; {:.1} s",
            sample.display(),
            outcome.rewritten,
            outcome.roblox_world,
            outcome.sheared,
            source_times.len(),
            changed.len(),
            started.elapsed().as_secs_f64()
        );
        assert!(changed.is_empty(), "the source was written: {changed:?}");
        assert!(missing.is_empty(), "parts missing after the move: {missing:?}");
        assert!(
            worst_position < 1e-3 && worst_rotation < 1e-3,
            "{worst_key} moved {worst_position} m, {worst_rotation} in rotation"
        );
    }
}
