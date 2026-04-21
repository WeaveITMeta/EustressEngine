// Universe helpers — tiny fs layer that maps Eustress's file-system-first
// conventions onto ergonomic tool inputs. Functions take pre-validated
// absolute paths; `resolve_in_universe` is the gatekeeper for path safety.

use serde::Serialize;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

// Cap result sizes so we never hand back megabytes to the client. MCP clients
// that blow past their natural bounds get truncated silently on their side;
// bounding up front gives us a clean "truncated" signal we can surface.
pub const MAX_LIST_ITEMS: usize = 500;
pub const MAX_SEARCH_MATCHES: usize = 200;
pub const MAX_FILE_BYTES: usize = 256 * 1024;

#[derive(Serialize, Debug, Clone)]
pub struct SpaceInfo {
    pub name: String,
    pub path: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct ScriptInfo {
    pub name: String,
    pub folder: String,
    pub space: String,
    pub class: String,
    pub source_path: String,
    pub summary_path: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct EntityMatch {
    pub name: String,
    pub class: String,
    pub space: String,
    pub path: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct SearchMatch {
    pub path: String,
    pub line: usize,
    pub preview: String,
}

pub fn list_spaces(universe: &Path) -> Vec<SpaceInfo> {
    let spaces_dir = universe.join("Spaces");
    let read = match std::fs::read_dir(&spaces_dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in read.flatten() {
        if let Ok(ft) = entry.file_type() {
            if ft.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                let path = entry.path().to_string_lossy().to_string();
                out.push(SpaceInfo { name, path });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn list_scripts(universe: &Path, space_filter: Option<&str>) -> Vec<ScriptInfo> {
    let spaces: Vec<SpaceInfo> = list_spaces(universe)
        .into_iter()
        .filter(|s| space_filter.map_or(true, |f| f == s.name))
        .collect();

    let mut out = Vec::new();
    for space in &spaces {
        walk_for_scripts(Path::new(&space.path), &space.name, &mut out);
        if out.len() >= MAX_LIST_ITEMS {
            break;
        }
    }
    out.truncate(MAX_LIST_ITEMS);
    out
}

fn walk_for_scripts(dir: &Path, space_name: &str, out: &mut Vec<ScriptInfo>) {
    if out.len() >= MAX_LIST_ITEMS {
        return;
    }
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    let entries: Vec<_> = read.flatten().collect();

    // Is this directory itself a script folder?
    let has_instance = entries
        .iter()
        .any(|e| e.file_name() == std::ffi::OsStr::new("_instance.toml"));
    if has_instance {
        if let Some(info) = maybe_read_as_script(dir, space_name) {
            out.push(info);
        }
    }

    for e in &entries {
        let ft = match e.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if !ft.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if name == ".eustress" || name.starts_with('.') {
            continue;
        }
        if out.len() >= MAX_LIST_ITEMS {
            return;
        }
        walk_for_scripts(&e.path(), space_name, out);
    }
}

fn maybe_read_as_script(folder: &Path, space: &str) -> Option<ScriptInfo> {
    let toml_path = folder.join("_instance.toml");
    let toml_text = std::fs::read_to_string(&toml_path).ok()?;
    let klass = extract_toml(&toml_text, "class_name")?;
    const SCRIPT_CLASSES: &[&str] = &["Script", "SoulScript", "LocalScript", "ModuleScript"];
    if !SCRIPT_CLASSES.contains(&klass.as_str()) {
        return None;
    }

    let name = folder.file_name()?.to_string_lossy().to_string();
    let source_path = find_script_source(folder, &name)?;
    let summary_path = find_script_summary(folder, &name);

    Some(ScriptInfo {
        name,
        folder: folder.to_string_lossy().to_string(),
        space: space.to_string(),
        class: klass,
        source_path: source_path.to_string_lossy().to_string(),
        summary_path: summary_path.map(|p| p.to_string_lossy().to_string()),
    })
}

fn find_script_source(folder: &Path, name: &str) -> Option<PathBuf> {
    let canonical = folder.join(format!("{name}.rune"));
    if canonical.exists() {
        return Some(canonical);
    }
    for entry in std::fs::read_dir(folder).ok()?.flatten() {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if !entry.file_type().ok()?.is_file() {
            continue;
        }
        if name_str.ends_with(".rune")
            || name_str.ends_with(".luau")
            || name_str.ends_with(".soul")
            || name_str.ends_with(".lua")
        {
            return Some(entry.path());
        }
    }
    None
}

fn find_script_summary(folder: &Path, name: &str) -> Option<PathBuf> {
    let canonical = folder.join(format!("{name}.md"));
    if canonical.exists() {
        return Some(canonical);
    }
    let legacy = folder.join("Summary.md");
    if legacy.exists() {
        return Some(legacy);
    }
    None
}

pub fn find_entity(universe: &Path, query: &str, space_filter: Option<&str>) -> Vec<EntityMatch> {
    let needle = query.to_lowercase();
    let spaces: Vec<SpaceInfo> = list_spaces(universe)
        .into_iter()
        .filter(|s| space_filter.map_or(true, |f| f == s.name))
        .collect();

    let mut out = Vec::new();
    for space in &spaces {
        walk_for_entities(Path::new(&space.path), &space.name, &needle, &mut out);
        if out.len() >= MAX_LIST_ITEMS {
            break;
        }
    }
    out.truncate(MAX_LIST_ITEMS);
    out
}

fn walk_for_entities(dir: &Path, space_name: &str, needle: &str, out: &mut Vec<EntityMatch>) {
    if out.len() >= MAX_LIST_ITEMS {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };

    for e in entries.flatten() {
        let ft = match e.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let name = e.file_name().to_string_lossy().to_string();
        if ft.is_dir() {
            if name == ".eustress" || name.starts_with('.') {
                continue;
            }
            walk_for_entities(&e.path(), space_name, needle, out);
            if out.len() >= MAX_LIST_ITEMS {
                return;
            }
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let is_instance = name == "_instance.toml";
        let is_flat = name.ends_with(".part.toml")
            || name.ends_with(".glb.toml")
            || name.ends_with(".model.toml")
            || name.ends_with(".textlabel.toml");
        if !is_instance && !is_flat {
            continue;
        }
        let full = e.path();
        let toml_text = match std::fs::read_to_string(&full) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let name_field = extract_toml(&toml_text, "name");
        let klass = extract_toml(&toml_text, "class_name").unwrap_or_else(|| "Instance".to_string());
        let identity = match name_field {
            Some(n) => n,
            None if is_instance => dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
            None => name.split('.').next().unwrap_or(&name).to_string(),
        };
        if !identity.to_lowercase().contains(needle) {
            continue;
        }
        out.push(EntityMatch {
            name: identity,
            class: klass,
            space: space_name.to_string(),
            path: full.to_string_lossy().to_string(),
        });
        if out.len() >= MAX_LIST_ITEMS {
            return;
        }
    }
}

pub fn search_universe(universe: &Path, query: &str, space_filter: Option<&str>) -> Vec<SearchMatch> {
    let needle = query.to_lowercase();
    let roots: Vec<PathBuf> = if let Some(space) = space_filter {
        list_spaces(universe)
            .into_iter()
            .filter(|s| s.name == space)
            .map(|s| PathBuf::from(s.path))
            .collect()
    } else {
        vec![universe.join("Spaces")]
    };

    let mut out = Vec::new();
    for root in &roots {
        walk_for_search(root, &needle, &mut out);
        if out.len() >= MAX_SEARCH_MATCHES {
            break;
        }
    }
    out.truncate(MAX_SEARCH_MATCHES);
    out
}

fn walk_for_search(dir: &Path, needle: &str, out: &mut Vec<SearchMatch>) {
    if out.len() >= MAX_SEARCH_MATCHES {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for e in entries.flatten() {
        let ft = match e.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let name = e.file_name().to_string_lossy().to_string();
        let abs = e.path();
        if ft.is_dir() {
            if name == ".eustress" || name.starts_with('.') {
                continue;
            }
            walk_for_search(&abs, needle, out);
            if out.len() >= MAX_SEARCH_MATCHES {
                return;
            }
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        if !(name.ends_with(".rune") || name.ends_with(".toml") || name.ends_with(".md")) {
            continue;
        }
        let text = match std::fs::read_to_string(&abs) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for (i, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(needle) {
                let preview = line.trim();
                let capped: String = preview.chars().take(200).collect();
                out.push(SearchMatch {
                    path: abs.to_string_lossy().to_string(),
                    line: i + 1,
                    preview: capped,
                });
                if out.len() >= MAX_SEARCH_MATCHES {
                    return;
                }
            }
        }
    }
}

/// Extract a scalar string field from a TOML file. Uses the real `toml`
/// parser so nested tables and mixed content parse correctly (improvement
/// over the TS version's per-line substring hack).
pub fn extract_toml(text: &str, field: &str) -> Option<String> {
    let value: toml::Value = toml::from_str(text).ok()?;
    extract_toml_recursive(&value, field)
}

fn extract_toml_recursive(v: &toml::Value, field: &str) -> Option<String> {
    match v {
        toml::Value::Table(t) => {
            if let Some(found) = t.get(field).and_then(|x| x.as_str()) {
                return Some(found.to_string());
            }
            for (_, nested) in t.iter() {
                if let Some(hit) = extract_toml_recursive(nested, field) {
                    return Some(hit);
                }
            }
            None
        }
        _ => None,
    }
}

pub struct CappedRead {
    pub text: String,
    pub truncated: bool,
}

pub fn read_capped(abs: &Path) -> std::io::Result<CappedRead> {
    let bytes = std::fs::read(abs)?;
    let truncated = bytes.len() > MAX_FILE_BYTES;
    let slice = if truncated { &bytes[..MAX_FILE_BYTES] } else { &bytes[..] };
    let text = String::from_utf8_lossy(slice).into_owned();
    Ok(CappedRead { text, truncated })
}

/// Path-safety gatekeeper. Resolves `input` against `universe`, rejects
/// anything that escapes the root. Returns the resolved absolute path on
/// success, `Err(msg)` on violation.
pub fn resolve_in_universe(universe: &Path, input: &str) -> Result<PathBuf, String> {
    let universe_abs = universe
        .canonicalize()
        .unwrap_or_else(|_| universe.to_path_buf());
    let input_path = Path::new(input);
    let resolved = if input_path.is_absolute() {
        input_path.to_path_buf()
    } else {
        universe_abs.join(input_path)
    };
    // Canonicalize if it exists; otherwise normalize components manually so we
    // can still reject `..` escapes for paths we're about to create.
    let normalized = match resolved.canonicalize() {
        Ok(p) => p,
        Err(_) => normalize_components(&resolved),
    };
    if !normalized.starts_with(&universe_abs) {
        return Err(format!("path '{input}' escapes the Universe root"));
    }
    Ok(normalized)
}

fn normalize_components(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Walk up from `start` looking for the enclosing Universe — any directory
/// with a `Spaces/` subdirectory. Bounded to 64 iterations so a pathological
/// cycle (Windows junction loops) can't hang us.
pub fn find_universe_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    let mut seen = HashSet::new();
    for _ in 0..64 {
        if !seen.insert(cur.clone()) {
            return None;
        }
        let spaces = cur.join("Spaces");
        if spaces.is_dir() {
            return Some(cur);
        }
        let parent = cur.parent()?.to_path_buf();
        if parent == cur {
            return None;
        }
        cur = parent;
    }
    None
}

/// Shallow scan of `roots` for Universes. Each root is itself checked, and
/// one level down is walked (matches how users organise: `~/Eustress/MyGame/`,
/// `~/Eustress/SideProject/`).
pub fn discover_universes(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = HashSet::new();
    for raw in roots {
        let root = raw.canonicalize().unwrap_or_else(|_| raw.clone());
        if root.join("Spaces").is_dir() {
            out.insert(root.clone());
        }
        let read = match std::fs::read_dir(&root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for e in read.flatten() {
            let ft = match e.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if !ft.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let child = root.join(&name);
            if child.join("Spaces").is_dir() {
                out.insert(child);
            }
        }
    }
    let mut v: Vec<PathBuf> = out.into_iter().collect();
    v.sort();
    v
}

/// Parse `EUSTRESS_UNIVERSES_PATH` (OS path-separator delimited).
pub fn parse_search_roots(env_val: Option<&str>) -> Vec<PathBuf> {
    if let Some(raw) = env_val {
        return std::env::split_paths(raw).collect();
    }
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        out.push(home.join("Eustress"));
        out.push(home.join("Documents").join("Eustress"));
        out.push(home);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_toml_top_level() {
        let src = "[metadata]\nclass_name = \"Script\"\nother = 1";
        assert_eq!(extract_toml(src, "class_name").as_deref(), Some("Script"));
    }

    #[test]
    fn extract_toml_missing() {
        let src = "[metadata]\nname = \"X\"";
        assert!(extract_toml(src, "class_name").is_none());
    }

    #[test]
    fn resolve_in_universe_rejects_escape() {
        let tmp = std::env::temp_dir();
        let u = tmp.join("eustress-test-universe");
        std::fs::create_dir_all(u.join("Spaces")).ok();
        let err = resolve_in_universe(&u, "../../etc/passwd");
        assert!(err.is_err());
    }
}
