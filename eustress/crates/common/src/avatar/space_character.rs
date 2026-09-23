//! A Space's own player characters.
//!
//! A Space replaces the engine's default bodies with files under
//! `StarterPlayer/Characters/`, one per body option:
//!
//! | File | Used for |
//! |---|---|
//! | `Masculine.rig.toml` (or `Male.rig.toml`) | accounts with the masculine body |
//! | `Feminine.rig.toml` (or `Female.rig.toml`) | accounts with the feminine body |
//! | `Robot.rig.toml` | agent accounts |
//! | `Default.rig.toml` | any body option without a file of its own |
//!
//! The body option belongs to the account (identity verification sets it)
//! and is never chosen here; a Space only decides what each option looks like
//! inside it.
//!
//! ```toml
//! label = "Box Head Hero"
//! body = "StarterPlayer/Characters/BoxheadHero.glb"   # inside the Space
//!
//! [animations]            # optional; a missing clip uses the engine's own
//! idle = "StarterPlayer/Characters/HeroIdle.glb"
//! walk = "StarterPlayer/Characters/HeroWalk.glb"
//!
//! [bones]                 # optional; node name in the file = standard bone
//! "Bip01 Pelvis" = "hips"
//! ```
//!
//! Paths are relative to the Space folder and may use letters, digits and
//! `_ - . /`; `bundled://characters/...` names one of the engine's own assets.
//! Each clip is `Animation0` of its GLB. Bones are matched by name
//! automatically (Mixamo names and the usual variants: hips or pelvis, spine,
//! spine1, spine2, neck, head, leftshoulder, leftarm, leftforearm, lefthand,
//! leftupleg, leftleg, leftfoot, lefttoebase and the right side); `[bones]`
//! maps anything else. Clips play on a body as authored for its bind pose, so
//! a body built on the Mixamo skeleton runs the engine's own clips unchanged.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::Deserialize;

use super::abilities::AvatarAbilities;
use eustress_avatar_schema::{AvatarDescriptor, AvatarIdentity, RigDefinition};

/// What the open Space asks of the avatars it spawns. The host inserts it
/// before requesting an avatar; without it the engine defaults apply.
#[derive(Resource, Debug, Clone, Default)]
pub struct SpaceCharacterPolicy {
    /// The Space folder. `space://` clips are read from here when they are
    /// retargeted (the asset server resolves `space://` on its own).
    pub space_root: Option<PathBuf>,
    /// A replacement rig per body option.
    pub rigs: HashMap<AvatarIdentity, RigDefinition>,
    /// For a body option without a replacement of its own.
    pub fallback: Option<RigDefinition>,
    /// The movement verbs every avatar starts with.
    pub abilities: AvatarAbilities,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CharacterFile {
    #[serde(default)]
    label: Option<String>,
    body: String,
    #[serde(default)]
    animations: CharacterClips,
    #[serde(default)]
    bones: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CharacterClips {
    idle: Option<String>,
    walk: Option<String>,
    run: Option<String>,
    jump: Option<String>,
}

/// The body option a file stem names: `Some(None)` for `Default`, `None` for
/// a name that is not an option.
fn option_of(stem: &str) -> Option<Option<AvatarIdentity>> {
    match stem.to_ascii_lowercase().as_str() {
        "masculine" | "male" => Some(Some(AvatarIdentity::Male)),
        "feminine" | "female" => Some(Some(AvatarIdentity::Female)),
        "robot" => Some(Some(AvatarIdentity::Robot)),
        "default" => Some(None),
        _ => None,
    }
}

/// A Space-relative path as an asset path; `bundled://` and `space://` pass
/// through.
fn asset_path(path: &str) -> String {
    let p = path.trim().replace('\\', "/");
    if p.starts_with("bundled://") || p.starts_with("space://") {
        p
    } else {
        format!("space://{}", p.trim_start_matches("./"))
    }
}

/// Empty clip slots take the engine's own clip for that body option.
fn fill_default_clips(rig: &mut RigDefinition, identity: AvatarIdentity) {
    let builtin = RigDefinition::builtin(identity);
    for (slot, clip) in rig.animations.iter_mut().enumerate() {
        if clip.is_empty() {
            *clip = builtin.animations[slot].clone();
        }
    }
}

/// The file behind a clip's asset path: `bundled://` from the engine's asset
/// root, `space://` from the Space folder.
pub fn clip_file(asset: &str, space_root: Option<&Path>) -> Option<PathBuf> {
    if let Some(rel) = asset.strip_prefix("bundled://") {
        return Some(super::boot::bundled_root().join(rel));
    }
    let rel = asset.strip_prefix("space://")?;
    Some(space_root?.join(rel))
}

impl SpaceCharacterPolicy {
    /// Read `<space>/StarterPlayer/Characters/*.rig.toml`. A file that does
    /// not parse or validate is skipped and named in the warnings, so one typo
    /// costs that body option rather than the whole Space.
    pub fn load(space_root: &Path) -> (Self, Vec<String>) {
        let mut policy = Self { space_root: Some(space_root.to_path_buf()), ..Default::default() };
        let mut warnings = Vec::new();
        let dir = space_root.join("StarterPlayer").join("Characters");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return (policy, warnings);
        };
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        files.sort();
        for path in files {
            let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
            let Some(stem) = name.strip_suffix(".rig.toml") else { continue };
            let Some(option) = option_of(stem) else {
                warnings.push(format!("{name}: name it Masculine, Feminine, Robot or Default"));
                continue;
            };
            match read_rig(&path, stem, option.unwrap_or(AvatarIdentity::Male)) {
                Ok(rig) => match option {
                    Some(identity) => {
                        policy.rigs.insert(identity, rig);
                    }
                    None => policy.fallback = Some(rig),
                },
                Err(e) => warnings.push(format!("{name}: {e}")),
            }
        }
        (policy, warnings)
    }

    /// The descriptor with this Space's body for its body option. Unchanged
    /// when the Space has none, or when the result would not validate.
    pub fn apply(&self, descriptor: AvatarDescriptor) -> AvatarDescriptor {
        let identity = descriptor.resolved_identity();
        let Some(rig) = self.rigs.get(&identity).or(self.fallback.as_ref()) else {
            return descriptor;
        };
        let mut rig = rig.clone();
        rig.identity = identity;
        fill_default_clips(&mut rig, identity);
        let mut out = descriptor.clone();
        out.rig = Some(rig);
        match out.validate() {
            Ok(()) => out,
            Err(e) => {
                warn!("avatar: the Space's {} character is unusable ({e}); using the default", identity.label());
                descriptor
            }
        }
    }
}

fn read_rig(path: &Path, stem: &str, identity: AvatarIdentity) -> Result<RigDefinition, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let file: CharacterFile = toml::from_str(&text).map_err(|e| e.to_string())?;
    let clip = |c: &Option<String>| c.as_deref().map(asset_path).unwrap_or_default();
    let rig = RigDefinition {
        id: format!("space-{}", stem.to_ascii_lowercase()),
        label: file.label.unwrap_or_else(|| stem.to_string()),
        identity,
        body_asset: asset_path(&file.body),
        // Empty = the engine's own clip for the player's body option, filled
        // in by `apply` once that option is known.
        animations: [
            clip(&file.animations.idle),
            clip(&file.animations.walk),
            clip(&file.animations.run),
            clip(&file.animations.jump),
        ],
        bone_aliases: file.bones.into_iter().map(|(from, to)| (from, to.to_ascii_lowercase())).collect(),
    };
    // Check it as it will be used: with the default clips in the gaps.
    let mut check = rig.clone();
    fill_default_clips(&mut check, identity);
    check.validate()?;
    Ok(rig)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space_with(files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "eustress-space-character-{}-{}",
            std::process::id(),
            files.len() * 7919 + files.first().map_or(0, |f| f.0.len())
        ));
        let dir = root.join("StarterPlayer").join("Characters");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in files {
            std::fs::write(dir.join(name), text).unwrap();
        }
        root
    }

    #[test]
    fn a_space_replaces_one_body_option_and_keeps_the_default_clips() {
        let root = space_with(&[("Masculine.rig.toml", "label = \"Hero\"\nbody = \"StarterPlayer/Characters/Hero.glb\"\n")]);
        let (policy, warnings) = SpaceCharacterPolicy::load(&root);
        assert!(warnings.is_empty(), "{warnings:?}");

        let male = policy.apply(AvatarDescriptor::default());
        let rig = male.resolved_rig();
        assert_eq!(rig.body_asset, "space://StarterPlayer/Characters/Hero.glb");
        assert_eq!(rig.animations, RigDefinition::builtin(AvatarIdentity::Male).animations);
        assert_eq!(rig.identity, AvatarIdentity::Male);

        // A body option the Space did not replace keeps the engine's body.
        let mut female = AvatarDescriptor::default();
        female.identity = AvatarIdentity::Female;
        female.base_body = eustress_avatar_schema::BaseBody::Feminine;
        assert_eq!(policy.apply(female.clone()).resolved_rig(), female.resolved_rig());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn default_covers_every_option_and_a_bad_file_is_reported_not_fatal() {
        let root = space_with(&[
            ("Default.rig.toml", "body = \"Hero.glb\"\n[animations]\nidle = \"HeroIdle.glb\"\n[bones]\n\"Bip01 Pelvis\" = \"Hips\"\n"),
            ("Feminine.rig.toml", "body = \"../outside.glb\"\n"),
            ("Wizard.rig.toml", "body = \"Wizard.glb\"\n"),
        ]);
        let (policy, warnings) = SpaceCharacterPolicy::load(&root);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        let rig = policy.apply(AvatarDescriptor::default()).resolved_rig();
        assert_eq!(rig.body_asset, "space://Hero.glb");
        assert_eq!(rig.animations[0], "space://HeroIdle.glb");
        assert_eq!(rig.animations[1], RigDefinition::builtin(AvatarIdentity::Male).animations[1]);
        assert_eq!(rig.bone_aliases, vec![("Bip01 Pelvis".to_string(), "hips".to_string())]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn clip_files_resolve_per_source() {
        let space = Path::new("C:/Spaces/Game");
        assert_eq!(clip_file("space://Anim/idle.glb", Some(space)), Some(space.join("Anim/idle.glb")));
        assert_eq!(clip_file("space://Anim/idle.glb", None), None);
        assert!(clip_file("bundled://characters/animations/male_idle.glb", None).is_some());
        assert_eq!(clip_file("https://example.com/idle.glb", Some(space)), None);
    }
}
