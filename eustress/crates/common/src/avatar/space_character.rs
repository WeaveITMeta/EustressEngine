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
    /// Heights (metres) the Space sets, by rig id.
    pub heights: HashMap<String, f32>,
    /// The movement verbs every avatar starts with.
    pub abilities: AvatarAbilities,
    /// The pace and jump every avatar starts with.
    pub movement: SpaceMovement,
}

/// The movement a Space sets for every character through StarterPlayer
/// (`CharacterWalkSpeed`, `CharacterJumpHeight`, `CharacterJumpPower` and
/// `CharacterUseJumpPower`), in metres. `None` leaves the avatar's own
/// body-derived value, so a Space that sets nothing keeps it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpaceMovement {
    /// The walking pace, m/s. Running and sprinting keep their ratio to it.
    pub walk_speed_mps: Option<f32>,
    /// The height a jump peaks at under the live gravity, metres.
    pub jump_height_m: Option<f32>,
    /// The take-off speed, m/s, used instead of the height when
    /// `use_jump_power` is set.
    pub jump_power_mps: Option<f32>,
    pub use_jump_power: bool,
}

/// The avatar's body-derived running pace over its walking pace
/// (`eustress_avatar_schema::resolve_motion`: 3.9 and 1.45 m/s, both scaled
/// by the stride), which a Space's walking pace keeps.
const RUN_OVER_WALK: f32 = 3.9 / 1.45;

impl SpaceMovement {
    /// `descriptor` at this Space's pace. StarterPlayer's values replace the
    /// avatar's own, as they set a Roblox character's Humanoid at spawn.
    pub fn apply_to(&self, mut descriptor: AvatarDescriptor) -> AvatarDescriptor {
        let motion = &mut descriptor.motion;
        if let Some(walk) = self.walk_speed_mps {
            let ratio = match (motion.walk_speed_mps, motion.run_speed_mps) {
                (Some(w), Some(r)) if w > 0.0 && r > 0.0 => r / w,
                _ => RUN_OVER_WALK,
            };
            motion.walk_speed_mps = Some(walk);
            motion.run_speed_mps = Some(walk * ratio);
        }
        if self.use_jump_power {
            if let Some(speed) = self.jump_power_mps {
                motion.jump_speed_mps = Some(speed);
            }
        } else if let Some(height) = self.jump_height_m {
            motion.jump_apex_m = Some(height);
        }
        descriptor
    }
}

/// A StarterPlayer property as its reader holds it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StarterValue {
    Number(f64),
    Bool(bool),
}

/// Numbers the StarterPlayer template once shipped untagged, in studs, by
/// property. Native Spaces copied them unchanged, so an untagged value equal
/// to one reads as unset and the engine's own default applies.
pub const STARTER_PLAYER_LEGACY_DEFAULTS: [(&str, f64); 7] = [
    ("CharacterWalkSpeed", 16.0),
    ("CharacterJumpHeight", 7.2),
    ("CharacterJumpPower", 50.0),
    ("NameDisplayDistance", 100.0),
    ("HealthDisplayDistance", 100.0),
    ("CameraMaxZoomDistance", 128.0),
    ("CameraMinZoomDistance", 0.5),
];

/// A Space's `StarterPlayer/_service.toml`.
pub fn starter_player_file(space_root: &Path) -> PathBuf {
    space_root.join("StarterPlayer").join("_service.toml")
}

/// A StarterPlayer length or speed in metres (per second). `value` is in the
/// file's `unit`, and a file that declares none reads in the stud. An
/// untagged value equal to the template's old default for `property` is
/// unset, as is a negative or non-finite one.
pub fn starter_player_metres(property: &str, value: f64, unit: Option<&str>) -> Option<f32> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    if unit.is_none() && STARTER_PLAYER_LEGACY_DEFAULTS.iter().any(|(p, d)| *p == property && *d == value) {
        return None;
    }
    let unit = unit.and_then(crate::units::Unit::from_symbol).unwrap_or(crate::units::Unit::Stud);
    Some((value * unit.to_meters()) as f32)
}

/// `ClimbingEnabled` as `_service.toml` names it: `climbing_enabled`.
fn snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CharacterFile {
    #[serde(default)]
    label: Option<String>,
    body: String,
    /// Metres; clamped to the engine's body range.
    #[serde(default)]
    height: Option<f32>,
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
    /// Read the Space's `StarterPlayer/_service.toml` (the movement verbs and
    /// movement every avatar starts with) and its
    /// `StarterPlayer/Characters/*.rig.toml`. A file that does not parse or
    /// validate is skipped and named in the warnings, so one typo costs that
    /// file rather than the whole Space.
    pub fn load(space_root: &Path) -> (Self, Vec<String>) {
        let mut policy = Self { space_root: Some(space_root.to_path_buf()), ..Default::default() };
        let mut warnings = Vec::new();
        if let Ok(text) = std::fs::read_to_string(starter_player_file(space_root)) {
            match text.parse::<toml::Table>() {
                Ok(doc) => {
                    let unit = doc
                        .get("metadata")
                        .and_then(toml::Value::as_table)
                        .and_then(|m| m.get("unit"))
                        .and_then(toml::Value::as_str);
                    // `[properties]`, the top level, or `[service]`: the first
                    // that holds a key wins.
                    let tables: Vec<&toml::Table> = [
                        doc.get("properties").and_then(toml::Value::as_table),
                        Some(&doc),
                        doc.get("service").and_then(toml::Value::as_table),
                    ]
                    .into_iter()
                    .flatten()
                    .collect();
                    let get = |key: &str| match tables.iter().find_map(|t| t.get(key)) {
                        Some(toml::Value::Float(f)) => Some(StarterValue::Number(*f)),
                        Some(toml::Value::Integer(i)) => Some(StarterValue::Number(*i as f64)),
                        Some(toml::Value::Boolean(b)) => Some(StarterValue::Bool(*b)),
                        _ => None,
                    };
                    warnings.extend(policy.read_starter_player(get, unit));
                }
                Err(e) => warnings.push(format!("StarterPlayer/_service.toml: {e}")),
            }
        }
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
                warnings.push(format!("StarterPlayer/Characters/{name}: name it Masculine, Feminine, Robot or Default"));
                continue;
            };
            match read_rig(&path, stem, option.unwrap_or(AvatarIdentity::Male)) {
                Ok((rig, height)) => {
                    if let Some(h) = height {
                        policy.heights.insert(rig.id.clone(), h);
                    }
                    match option {
                    Some(identity) => {
                        policy.rigs.insert(identity, rig);
                    }
                        None => policy.fallback = Some(rig),
                    }
                }
                Err(e) => warnings.push(format!("StarterPlayer/Characters/{name}: {e}")),
            }
        }
        (policy, warnings)
    }

    /// Take StarterPlayer's movement verbs and movement from its properties,
    /// which `get` looks up by their file names (`climbing_enabled`) or their
    /// Roblox names (`ClimbingEnabled`). `unit` is the file's declared unit,
    /// `None` when it declares none. Returns what it could not read.
    pub fn read_starter_player(&mut self, get: impl Fn(&str) -> Option<StarterValue>, unit: Option<&str>) -> Vec<String> {
        let mut warnings = Vec::new();
        let unit = match unit {
            Some(u) if crate::units::Unit::from_symbol(u).is_none() => {
                warnings.push(format!("StarterPlayer/_service.toml: unknown unit {u:?}; its values read in the stud"));
                Some("stud")
            }
            u => u,
        };
        let look = |name: &str| get(&snake_case(name)).or_else(|| get(name));
        for name in AvatarAbilities::PROPERTIES {
            if let Some(StarterValue::Bool(on)) = look(name) {
                self.abilities.set(name, on);
            }
        }
        let metres = |name: &str| match look(name) {
            Some(StarterValue::Number(v)) => starter_player_metres(name, v, unit),
            _ => None,
        };
        self.movement = SpaceMovement {
            walk_speed_mps: metres("CharacterWalkSpeed"),
            jump_height_m: metres("CharacterJumpHeight"),
            jump_power_mps: metres("CharacterJumpPower"),
            use_jump_power: look("CharacterUseJumpPower") == Some(StarterValue::Bool(true)),
        };
        warnings
    }

    /// The descriptor at this Space's pace, with this Space's body for its
    /// body option. The body is unchanged when the Space has none, or when the
    /// result would not validate.
    pub fn apply(&self, descriptor: AvatarDescriptor) -> AvatarDescriptor {
        let descriptor = self.movement.apply_to(descriptor);
        let identity = descriptor.resolved_identity();
        let Some(rig) = self.rigs.get(&identity).or(self.fallback.as_ref()) else {
            return descriptor;
        };
        let mut rig = rig.clone();
        rig.identity = identity;
        fill_default_clips(&mut rig, identity);
        let mut out = descriptor.clone();
        if let (Some(h), false) = (self.heights.get(&rig.id), identity == AvatarIdentity::Robot) {
            use eustress_avatar_schema::{MAX_HEIGHT_M, MIN_HEIGHT_M};
            out.morphs.height = eustress_avatar_schema::Norm01::new((h - MIN_HEIGHT_M) / (MAX_HEIGHT_M - MIN_HEIGHT_M));
        }
        out.rig = Some(rig);
        match out.validate() {
            Ok(()) => out,
            Err(e) => {
                tracing::warn!("avatar: the Space's {} character is unusable ({e}); using the default", identity.label());
                descriptor
            }
        }
    }
}

fn read_rig(path: &Path, stem: &str, identity: AvatarIdentity) -> Result<(RigDefinition, Option<f32>), String> {
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
    Ok((rig, file.height.filter(|h| h.is_finite() && *h > 0.0)))
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

    /// A Space holding only `StarterPlayer/_service.toml` with `text`.
    fn starter_player_space(tag: &str, text: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("eustress-starter-player-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("StarterPlayer")).unwrap();
        std::fs::write(starter_player_file(&root), text).unwrap();
        root
    }

    fn stud() -> f32 {
        crate::units::Unit::Stud.to_meters() as f32
    }

    #[test]
    fn an_untouched_template_copy_sets_nothing_and_its_switches_still_apply() {
        let root = starter_player_space(
            "template",
            "[properties]\ncharacter_walk_speed = 16.0\ncharacter_jump_height = 7.2\ncharacter_jump_power = 50.0\n\
             character_use_jump_power = false\nclimbing_enabled = false\n",
        );
        let (policy, warnings) = SpaceCharacterPolicy::load(&root);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(policy.movement, SpaceMovement::default(), "the body-derived pace stands");
        assert_eq!(policy.abilities.get("ClimbingEnabled"), Some(false));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_space_s_own_values_read_through_its_unit() {
        // Untagged reads in the stud; a value other than the old default is the Space's own.
        let root = starter_player_space("untagged", "[properties]\ncharacter_walk_speed = 24.0\n");
        let walk = SpaceCharacterPolicy::load(&root).0.movement.walk_speed_mps.unwrap();
        assert!((walk - 24.0 * stud()).abs() < 1e-5);
        let _ = std::fs::remove_dir_all(root);

        // Tagged "stud", even Roblox's default is the place's own choice.
        let root = starter_player_space(
            "stud",
            "[metadata]\nunit = \"stud\"\n[properties]\ncharacter_walk_speed = 16.0\ncharacter_jump_height = 7.2\n",
        );
        let movement = SpaceCharacterPolicy::load(&root).0.movement;
        assert!((movement.walk_speed_mps.unwrap() - 16.0 * stud()).abs() < 1e-5);
        assert!((movement.jump_height_m.unwrap() - 7.2 * stud()).abs() < 1e-5);
        let _ = std::fs::remove_dir_all(root);

        let root = starter_player_space(
            "metres",
            "[metadata]\nunit = \"m\"\n[properties]\ncharacter_walk_speed = 8\ncharacter_jump_power = 12.5\n\
             character_use_jump_power = true\n",
        );
        let movement = SpaceCharacterPolicy::load(&root).0.movement;
        assert_eq!(movement.walk_speed_mps, Some(8.0));
        assert_eq!(movement.jump_power_mps, Some(12.5));
        assert!(movement.use_jump_power);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_space_s_pace_replaces_the_avatars_and_running_keeps_its_ratio() {
        let movement = SpaceMovement { walk_speed_mps: Some(8.0), jump_height_m: Some(2.0), ..Default::default() };
        let d = movement.apply_to(AvatarDescriptor::default());
        assert_eq!(d.motion.walk_speed_mps, Some(8.0));
        assert!((d.motion.run_speed_mps.unwrap() - 8.0 * RUN_OVER_WALK).abs() < 1e-4);
        assert_eq!(d.motion.jump_apex_m, Some(2.0));

        // Jump power in force: the height is not the Space's jump.
        let power = SpaceMovement { jump_height_m: Some(2.0), jump_power_mps: Some(9.0), use_jump_power: true, ..Default::default() };
        let powered = power.apply_to(AvatarDescriptor::default()).motion;
        assert_eq!(powered.jump_apex_m, AvatarDescriptor::default().motion.jump_apex_m);
        assert_eq!(powered.jump_speed_mps, Some(9.0), "the take-off speed, never a height baked from it");

        // A Space with no body of its own still sets the pace.
        let policy = SpaceCharacterPolicy { movement, ..Default::default() };
        assert_eq!(policy.apply(AvatarDescriptor::default()).motion.walk_speed_mps, Some(8.0));
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
