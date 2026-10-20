//! # Where clips come from
//!
//! An `Animation`'s `AnimationId` names a clip by where it lives:
//!
//! | AnimationId | Clip |
//! |---|---|
//! | `space://Characters/Wave.glb#Animation1` | a glTF clip in the Space (`Animation0` with no fragment) |
//! | `bundled://characters/animations/male_walking.glb` | a clip shipped with Eustress |
//! | `space://ReplicatedStorage/Animations/Wave` | the `KeyframeSequence` at that path |
//! | `space://assets/animations/rbx-507770239.anim.toml` | a clip file |
//! | `rig://walk` | the clip the Animator's rig names `walk` |
//! | `active://3` | the sequence a script registered with `KeyframeSequenceProvider:RegisterKeyframeSequence` |
//! | `rbxassetid://507770239`, `http://www.roblox.com/asset/?id=507770239` | the file the Space's Roblox id map names |
//!
//! `space://` paths stay inside the Space: a `..` segment, a drive or a
//! leading slash is refused.

use std::path::{Path, PathBuf};

use crate::datamodel::{DataModel, InstanceId};

/// The Space's map from Roblox asset ids to files, written by the importer.
pub const ROBLOX_ID_MAP: &str = "assets/roblox_ids.toml";

/// The body option's clip slots a `rig://` id may name, in
/// `RigDefinition.animations` order.
pub const RIG_SLOTS: [&str; 4] = ["idle", "walk", "run", "jump"];

/// A clip's source, resolved.
#[derive(Debug, Clone, PartialEq)]
pub enum ClipSource {
    /// A glTF animation: the asset path to load (with its `#AnimationN`
    /// label) and, when known, the file behind it for retargeting.
    Gltf { asset_path: String, file: Option<PathBuf> },
    /// A `KeyframeSequence` record on disk.
    Record { file: PathBuf },
    /// A `KeyframeSequence` in the live tree.
    Live { instance: InstanceId },
}

/// The rig's own clips, for `rig://`: its four slots and any extra names a
/// Space's `*.rig.toml` gives.
#[derive(Debug, Clone, Default)]
pub struct RigClips {
    pub slots: [String; 4],
    pub extra: Vec<(String, String)>,
}

impl RigClips {
    pub fn get(&self, name: &str) -> Option<&str> {
        let lower = name.to_ascii_lowercase();
        if let Some(i) = RIG_SLOTS.iter().position(|s| *s == lower) {
            return Some(self.slots[i].as_str()).filter(|s| !s.is_empty());
        }
        self.extra.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, p)| p.as_str())
    }
}

/// Where the resolver looks.
pub struct Roots<'a> {
    /// The Space folder (Studio's open Space, the Player's world folder).
    pub space: Option<&'a Path>,
    /// Eustress's own asset folder.
    pub bundled: &'a Path,
}

/// Resolve `content` for a rig with `rig_clips` (None for a rig with no
/// body option, such as a `Motor6D` model).
pub fn resolve(content: &str, tree: &DataModel, rig_clips: Option<&RigClips>, roots: &Roots) -> Result<ClipSource, String> {
    let content = content.trim();
    if content.is_empty() {
        return Err("the Animation has no AnimationId".into());
    }
    if let Some(name) = content.strip_prefix("rig://") {
        let clips = rig_clips.ok_or_else(|| format!("{content} names the avatar's clip, and this rig has no avatar"))?;
        let path = clips.get(name).ok_or_else(|| format!("this rig has no clip named {name}"))?;
        if path.starts_with("rig://") {
            return Err(format!("{content} points at another rig:// id"));
        }
        return resolve(path, tree, None, roots);
    }
    if let Some(rel) = content.strip_prefix("bundled://") {
        let (path, label) = split_label(rel);
        let path = safe_relative(path).ok_or_else(|| format!("{content} leaves the bundled assets"))?;
        return Ok(gltf_or_record(&format!("bundled://{path}"), label, Some(roots.bundled.join(&path))));
    }
    if let Some(rel) = content.strip_prefix("space://") {
        let (path, label) = split_label(rel);
        let path = safe_relative(path).ok_or_else(|| format!("{content} leaves the Space"))?;
        if is_gltf(&path) || path.ends_with(".toml") {
            return Ok(gltf_or_record(&format!("space://{path}"), label, roots.space.map(|r| r.join(&path))));
        }
        // A path to an instance: the live KeyframeSequence, else its record.
        if let Some(instance) = find_path(tree, &path) {
            if tree.class_of(instance) == Some("KeyframeSequence") {
                return Ok(ClipSource::Live { instance });
            }
            return Err(format!("{content} is a {}, not a KeyframeSequence", tree.class_of(instance).unwrap_or("?")));
        }
        let root = roots.space.ok_or_else(|| format!("{content}: no Space folder to read from"))?;
        let record = root.join(&path).join("_instance.toml");
        if record.is_file() {
            return Ok(ClipSource::Record { file: record });
        }
        return Err(format!("{content} names nothing in this Space"));
    }
    if content.starts_with("active://") {
        return tree
            .registered_sequence(content)
            .map(|instance| ClipSource::Live { instance })
            .ok_or_else(|| format!("{content} names no registered KeyframeSequence"));
    }
    if let Some(id) = roblox_asset_id(content) {
        let root = roots.space.ok_or_else(|| format!("{content}: no Space folder holds a Roblox id map"))?;
        let mapped = roblox_id_file(root, id)?;
        return resolve(&format!("space://{mapped}"), tree, None, roots);
    }
    Err(format!(
        "{content} is not an AnimationId this Space can play (space://, bundled://, rig://, active:// or a Roblox \
         asset id)"
    ))
}

fn split_label(path: &str) -> (&str, Option<&str>) {
    match path.split_once('#') {
        Some((p, label)) => (p, Some(label)),
        None => (path, None),
    }
}

fn is_gltf(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".glb") || lower.ends_with(".gltf")
}

fn gltf_or_record(asset: &str, label: Option<&str>, file: Option<PathBuf>) -> ClipSource {
    if is_gltf(asset) {
        let label = label.filter(|l| !l.is_empty()).unwrap_or("Animation0");
        ClipSource::Gltf { asset_path: format!("{asset}#{label}"), file }
    } else {
        match file {
            Some(file) => ClipSource::Record { file },
            None => ClipSource::Gltf { asset_path: asset.to_string(), file: None },
        }
    }
}

/// A Space-relative path with forward slashes, or `None` if it would leave
/// the folder it is relative to.
pub fn safe_relative(path: &str) -> Option<String> {
    let p = path.trim().replace('\\', "/");
    let p = p.trim_start_matches("./");
    if p.is_empty() || p.starts_with('/') || p.contains(':') {
        return None;
    }
    let mut parts = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => continue,
            ".." => return None,
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

/// The instance a Space path names: its first segment a service, the rest
/// children by name.
pub fn find_path(tree: &DataModel, path: &str) -> Option<InstanceId> {
    let mut segs = path.split('/').filter(|s| !s.is_empty());
    let service = segs.next()?;
    let mut cur = tree.find_service(service)?;
    for seg in segs {
        cur = tree.find_first_child(cur, seg, false)?;
    }
    Some(cur)
}

/// The numeric id in any of the forms Roblox writes asset ids in.
pub fn roblox_asset_id(content: &str) -> Option<u64> {
    let lower = content.trim().to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("rbxassetid://") {
        return leading_number(rest);
    }
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.") {
        if !lower.contains("roblox.com") {
            return None;
        }
        if let Some(i) = lower.find("id=") {
            return leading_number(&lower[i + 3..]);
        }
    }
    None
}

fn leading_number(s: &str) -> Option<u64> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// The Space-relative file the Roblox id map names for `id`.
pub fn roblox_id_file(space_root: &Path, id: u64) -> Result<String, String> {
    let map_path = space_root.join(ROBLOX_ID_MAP);
    let text = std::fs::read_to_string(&map_path).map_err(|_| {
        format!(
            "rbxassetid://{id} has no clip in this Space: {ROBLOX_ID_MAP} is missing. Re-import the place with a \
             Roblox credential, or add the id to {ROBLOX_ID_MAP}"
        )
    })?;
    let doc: toml::Value = toml::from_str(&text).map_err(|e| format!("{ROBLOX_ID_MAP}: {e}"))?;
    let key = id.to_string();
    let entry = doc
        .get("assets")
        .and_then(|a| a.get(&key))
        .or_else(|| doc.get(&key))
        .ok_or_else(|| {
            format!("rbxassetid://{id} has no clip in this Space. Re-import with a Roblox credential, or add it to {ROBLOX_ID_MAP}")
        })?;
    let file = match entry {
        toml::Value::String(s) => s.clone(),
        toml::Value::Table(t) => t
            .get("file")
            .and_then(|f| f.as_str())
            .map(str::to_string)
            .ok_or_else(|| format!("{ROBLOX_ID_MAP}: the entry for {id} has no file"))?,
        _ => return Err(format!("{ROBLOX_ID_MAP}: the entry for {id} is not a file")),
    };
    safe_relative(&file).ok_or_else(|| format!("{ROBLOX_ID_MAP}: the file for {id} leaves the Space"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> (PathBuf, PathBuf) {
        (PathBuf::from("C:/Spaces/Game"), PathBuf::from("C:/Eustress/assets"))
    }

    #[test]
    fn glb_ids_get_their_default_label_and_file() {
        let (space, bundled) = roots();
        let r = Roots { space: Some(&space), bundled: &bundled };
        let tree = DataModel::new();
        match resolve("space://Characters/Wave.glb", &tree, None, &r).unwrap() {
            ClipSource::Gltf { asset_path, file } => {
                assert_eq!(asset_path, "space://Characters/Wave.glb#Animation0");
                assert_eq!(file, Some(space.join("Characters/Wave.glb")));
            }
            other => panic!("{other:?}"),
        }
        match resolve("bundled://characters/animations/male_walking.glb#Animation2", &tree, None, &r).unwrap() {
            ClipSource::Gltf { asset_path, .. } => {
                assert_eq!(asset_path, "bundled://characters/animations/male_walking.glb#Animation2")
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rig_ids_resolve_through_the_rigs_clips() {
        let (space, bundled) = roots();
        let r = Roots { space: Some(&space), bundled: &bundled };
        let tree = DataModel::new();
        let clips = RigClips {
            slots: [
                "bundled://characters/animations/male_idle.glb".into(),
                "bundled://characters/animations/male_walking.glb".into(),
                String::new(),
                String::new(),
            ],
            extra: vec![("wave".into(), "space://Anim/Wave.anim.toml".into())],
        };
        assert!(matches!(resolve("rig://walk", &tree, Some(&clips), &r), Ok(ClipSource::Gltf { .. })));
        assert!(matches!(resolve("rig://wave", &tree, Some(&clips), &r), Ok(ClipSource::Record { .. })));
        assert!(resolve("rig://run", &tree, Some(&clips), &r).is_err(), "an empty slot names nothing");
        assert!(resolve("rig://walk", &tree, None, &r).is_err(), "a rig with no avatar has no rig:// clips");
    }

    #[test]
    fn paths_that_leave_the_space_are_refused() {
        for bad in ["../x.glb", "a/../../x.glb", "/abs.glb", "C:/x.glb", ""] {
            assert_eq!(safe_relative(bad), None, "{bad}");
        }
        assert_eq!(safe_relative("./a\\b/./c.glb").as_deref(), Some("a/b/c.glb"));
        let (space, bundled) = roots();
        let r = Roots { space: Some(&space), bundled: &bundled };
        assert!(resolve("space://../../secret.toml", &DataModel::new(), None, &r).is_err());
    }

    #[test]
    fn every_roblox_url_form_yields_its_id() {
        assert_eq!(roblox_asset_id("rbxassetid://507770239"), Some(507770239));
        assert_eq!(roblox_asset_id("http://www.roblox.com/asset/?id=507766666"), Some(507766666));
        assert_eq!(roblox_asset_id("http://www.roblox.com/Asset?ID=132193066130399"), Some(132193066130399));
        assert_eq!(roblox_asset_id("https://example.com/?id=5"), None);
        assert_eq!(roblox_asset_id("space://x.glb"), None);
    }

    #[test]
    fn a_path_finds_a_live_keyframe_sequence() {
        let mut tree = DataModel::new();
        let rs = tree.get_service("ReplicatedStorage").unwrap();
        let folder = tree.create_virtual("Folder", "Animations", Some(rs));
        let ks = tree.create_virtual("KeyframeSequence", "Wave", Some(folder));
        let (space, bundled) = roots();
        let r = Roots { space: Some(&space), bundled: &bundled };
        assert_eq!(
            resolve("space://ReplicatedStorage/Animations/Wave", &tree, None, &r).unwrap(),
            ClipSource::Live { instance: ks }
        );
    }

    #[test]
    fn a_registered_sequence_plays_by_its_active_id() {
        let mut tree = DataModel::new();
        let ks = tree.create_virtual("KeyframeSequence", "Built", None);
        let id = tree.register_keyframe_sequence(ks).unwrap();
        assert_eq!(tree.register_keyframe_sequence(ks).unwrap(), id, "one sequence keeps its id");
        let (space, bundled) = roots();
        let r = Roots { space: Some(&space), bundled: &bundled };
        assert_eq!(resolve(&id, &tree, None, &r).unwrap(), ClipSource::Live { instance: ks });
        let part = tree.create_virtual("Part", "P", None);
        assert!(tree.register_keyframe_sequence(part).is_err());
        tree.destroy(ks);
        assert!(resolve(&id, &tree, None, &r).is_err(), "a destroyed sequence names nothing");
    }
}
