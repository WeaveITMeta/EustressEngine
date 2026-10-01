//! Work a rebuild keeps across a clean re-import.
//!
//! `rbx_import --clean` moves the old Space to `Spaces/.trash` and imports the
//! place fresh. A Space rebuilt on top of its import keeps two things of its
//! own, and [`carry_over`] brings both across:
//!
//! - every folder named `Rebuild` directly under a top-level folder
//!   (`Workspace/Rebuild`, `ServerScriptService/Rebuild`, ...), copied whole
//!   from the old Space; and
//! - `.eustress/rebuild_patches.toml`, the rebuild's edits to imported
//!   instances, copied and replayed onto the fresh files:
//!
//! ```toml
//! [[patch]]
//! path = "Workspace/Cars/Sedan"   # the instance folder, Space-relative
//! key = "attributes.MaxSpeed"     # a dotted key into its _instance.toml
//! value = 120                     # any TOML value
//!
//! [[patch]]
//! path = "ServerScriptService/CarSpawner"
//! key = "script.enabled"
//! value = false
//!
//! [[patch]]
//! path = "Workspace/Cars/Sedan"
//! key = "attributes.Legacy"
//! remove = true                   # instead of a value: delete the key
//! ```
//!
//! Key segments are split on `.`; a segment cannot itself hold a dot. A patch
//! whose instance no longer exists, or that cannot be applied, is written to
//! `.eustress/rebuild_patches_orphaned.toml` and counted, never dropped
//! silently.

use std::collections::BTreeMap;
use std::path::Path;

/// The folder name a rebuild keeps its own work under, one per service.
pub const REBUILD_FOLDER: &str = "Rebuild";
/// The rebuild's edits to imported instances, under `<Space>/.eustress/`.
pub const PATCH_FILE: &str = "rebuild_patches.toml";
/// The patches a replay could not apply, under `<Space>/.eustress/`.
pub const ORPHAN_FILE: &str = "rebuild_patches_orphaned.toml";

/// What [`carry_over`] brought across.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CarryOver {
    /// The `Rebuild` folders copied, Space-relative (`Workspace/Rebuild`).
    pub folders: Vec<String>,
    /// Patches written onto the fresh files.
    pub patches_applied: usize,
    /// Patches whose instance folder has no `_instance.toml` any more.
    pub patches_orphaned: usize,
    /// Patches that could not be read or applied (a missing field, a key
    /// through a value that is not a table).
    pub patches_invalid: usize,
}

/// Bring a rebuild's own work from `old_space` (the trashed copy) into
/// `new_space` (the fresh import). See the module docs.
pub fn carry_over(old_space: &Path, new_space: &Path) -> Result<CarryOver, String> {
    let mut out = CarryOver::default();

    // Rebuild folders, one level under each top-level folder.
    let entries = std::fs::read_dir(old_space).map_err(|e| format!("read {}: {e}", old_space.display()))?;
    let mut tops: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|name| !name.starts_with('.'))
        .collect();
    tops.sort();
    for top in tops {
        let from = old_space.join(&top).join(REBUILD_FOLDER);
        if from.is_dir() {
            let to = new_space.join(&top).join(REBUILD_FOLDER);
            copy_dir_all(&from, &to).map_err(|e| format!("copy {}: {e}", from.display()))?;
            out.folders.push(format!("{top}/{REBUILD_FOLDER}"));
        }
    }

    // The patch file: copied, then replayed.
    let patch_file = old_space.join(".eustress").join(PATCH_FILE);
    if !patch_file.is_file() {
        return Ok(out);
    }
    let text = std::fs::read_to_string(&patch_file).map_err(|e| format!("read {}: {e}", patch_file.display()))?;
    let dot = new_space.join(".eustress");
    std::fs::create_dir_all(&dot).map_err(|e| format!("mkdir {}: {e}", dot.display()))?;
    std::fs::write(dot.join(PATCH_FILE), &text).map_err(|e| format!("write {PATCH_FILE}: {e}"))?;

    let doc: toml::Value = text.parse().map_err(|e| format!("parse {PATCH_FILE}: {e}"))?;
    let patches: Vec<toml::Value> = doc
        .get("patch")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();

    // Group by instance file so each is read and written once, in file order.
    let mut by_path: BTreeMap<String, Vec<(usize, toml::Value)>> = BTreeMap::new();
    let mut orphans: Vec<toml::Value> = Vec::new();
    for (i, patch) in patches.into_iter().enumerate() {
        match patch.get("path").and_then(|p| p.as_str()) {
            Some(path) if !path.trim().is_empty() => {
                by_path.entry(path.trim_matches('/').to_string()).or_default().push((i, patch));
            }
            _ => {
                out.patches_invalid += 1;
                orphans.push(with_reason(patch, "no path"));
            }
        }
    }
    for (path, group) in by_path {
        let file = new_space.join(&path).join("_instance.toml");
        let parsed = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| t.parse::<toml::Value>().ok());
        let Some(mut instance) = parsed else {
            for (_, patch) in group {
                out.patches_orphaned += 1;
                orphans.push(with_reason(patch, "the instance is not in the fresh import"));
            }
            continue;
        };
        let mut changed = false;
        for (_, patch) in group {
            match apply_patch(&mut instance, &patch) {
                Ok(()) => {
                    out.patches_applied += 1;
                    changed = true;
                }
                Err(reason) => {
                    out.patches_invalid += 1;
                    orphans.push(with_reason(patch, &reason));
                }
            }
        }
        if changed {
            let text = toml::to_string_pretty(&instance).map_err(|e| format!("serialize {}: {e}", file.display()))?;
            std::fs::write(&file, text).map_err(|e| format!("write {}: {e}", file.display()))?;
        }
    }

    if !orphans.is_empty() {
        let mut table = toml::value::Table::new();
        table.insert("patch".to_string(), toml::Value::Array(orphans));
        let text = toml::to_string_pretty(&toml::Value::Table(table)).map_err(|e| format!("serialize {ORPHAN_FILE}: {e}"))?;
        std::fs::write(dot.join(ORPHAN_FILE), text).map_err(|e| format!("write {ORPHAN_FILE}: {e}"))?;
    }
    Ok(out)
}

/// Set or remove one dotted key of an instance document.
fn apply_patch(instance: &mut toml::Value, patch: &toml::Value) -> Result<(), String> {
    let key = patch
        .get("key")
        .and_then(|k| k.as_str())
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| "no key".to_string())?;
    let remove = patch.get("remove").and_then(|r| r.as_bool()).unwrap_or(false);
    let value = patch.get("value").cloned();
    if !remove && value.is_none() {
        return Err("neither value nor remove = true".to_string());
    }
    let segments: Vec<&str> = key.split('.').collect();
    let (last, parents) = segments.split_last().ok_or_else(|| "empty key".to_string())?;
    let mut table = instance.as_table_mut().ok_or_else(|| "the instance file is not a table".to_string())?;
    for seg in parents {
        if remove && !table.contains_key(*seg) {
            return Ok(()); // nothing to remove
        }
        let next = table
            .entry(seg.to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        table = next
            .as_table_mut()
            .ok_or_else(|| format!("`{seg}` in `{key}` is a value, not a table"))?;
    }
    if remove {
        table.remove(*last);
    } else if let Some(v) = value {
        table.insert(last.to_string(), v);
    }
    Ok(())
}

fn with_reason(mut patch: toml::Value, reason: &str) -> toml::Value {
    if let Some(t) = patch.as_table_mut() {
        t.insert("orphaned".to_string(), toml::Value::String(reason.to_string()));
    }
    patch
}

fn copy_dir_all(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "eustress_carry_over_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn rebuild_folders_and_patches_survive_a_clean_reimport() {
        let old = temp("old");
        let new = temp("new");
        // The rebuild's own work in the old Space.
        write(&old.join("Workspace/Rebuild/Chassis/_instance.toml"), "[metadata]\nclass_name = \"Model\"\n");
        write(&old.join("ServerScriptService/Rebuild/Drive/script.luau"), "print('drive')\n");
        write(&old.join("Workspace/Cars/_instance.toml"), "[metadata]\nclass_name = \"Folder\"\n");
        write(
            &old.join(".eustress").join(PATCH_FILE),
            "[[patch]]\npath = \"Workspace/Cars/Sedan\"\nkey = \"attributes.MaxSpeed\"\nvalue = 120\n\n\
             [[patch]]\npath = \"Workspace/Cars/Sedan\"\nkey = \"attributes.Legacy\"\nremove = true\n\n\
             [[patch]]\npath = \"ServerScriptService/OldSpawner\"\nkey = \"script.enabled\"\nvalue = false\n\n\
             [[patch]]\npath = \"Workspace/Gone\"\nkey = \"attributes.X\"\nvalue = 1\n",
        );
        // The fresh import.
        write(
            &new.join("Workspace/Cars/Sedan/_instance.toml"),
            "[metadata]\nclass_name = \"Model\"\n\n[attributes]\nMaxSpeed = 80\nLegacy = true\n",
        );
        write(&new.join("ServerScriptService/OldSpawner/_instance.toml"), "[metadata]\nclass_name = \"LuauScript\"\n\n[script]\nenabled = true\n");

        let got = carry_over(&old, &new).expect("carry over");
        assert_eq!(got.folders, vec!["ServerScriptService/Rebuild".to_string(), "Workspace/Rebuild".to_string()]);
        assert!(new.join("Workspace/Rebuild/Chassis/_instance.toml").is_file());
        assert!(new.join("ServerScriptService/Rebuild/Drive/script.luau").is_file());
        assert_eq!((got.patches_applied, got.patches_orphaned, got.patches_invalid), (3, 1, 0));

        let sedan: toml::Value = std::fs::read_to_string(new.join("Workspace/Cars/Sedan/_instance.toml")).unwrap().parse().unwrap();
        assert_eq!(sedan["attributes"]["MaxSpeed"].as_integer(), Some(120));
        assert!(sedan["attributes"].get("Legacy").is_none());
        let spawner: toml::Value = std::fs::read_to_string(new.join("ServerScriptService/OldSpawner/_instance.toml")).unwrap().parse().unwrap();
        assert_eq!(spawner["script"]["enabled"].as_bool(), Some(false));

        assert!(new.join(".eustress").join(PATCH_FILE).is_file(), "the patch file travels too");
        let orphans: toml::Value = std::fs::read_to_string(new.join(".eustress").join(ORPHAN_FILE)).unwrap().parse().unwrap();
        let listed = orphans["patch"].as_array().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["path"].as_str(), Some("Workspace/Gone"));

        let _ = std::fs::remove_dir_all(&old);
        let _ = std::fs::remove_dir_all(&new);
    }

    #[test]
    fn a_key_through_a_value_is_reported_not_applied() {
        let mut doc: toml::Value = "[metadata]\nname = \"Car\"\n".parse().unwrap();
        let patch: toml::Value = "path = \"x\"\nkey = \"metadata.name.first\"\nvalue = 1\n".parse().unwrap();
        assert!(apply_patch(&mut doc, &patch).is_err());
        assert_eq!(doc["metadata"]["name"].as_str(), Some("Car"));
    }
}
