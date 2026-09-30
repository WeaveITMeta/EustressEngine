//! Properties panel for the Sound class.
//!
//! Rows come from the live `Sound` component, so the panel shows the Sound's
//! own fields, not a Part's Appearance and Physics. An edit is parsed,
//! written to the component (a previewing Sound follows it the same frame,
//! through the shared player), saved into the file's `[sound]` section and
//! pushed on the undo stack as a `ChangeClassField`, whose replay comes back
//! through [`replay_field`]. The roll-off distances are shown in the display
//! unit and stored in metres.
//!
//! Preview plays the Sound in Edit, where a Sound never starts by itself; it
//! is not saved, and a Play session stops it. Playing is what the Sound does
//! when a session starts.
//!
//! A Sound in folder form has no `InstanceFile`; its file is
//! `<folder>/_instance.toml` ([`sound_file`]). Edits save during Play too:
//! nothing restores a Sound's fields on Stop.

use std::path::{Path, PathBuf};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use eustress_common::classes::{ClassName, Sound};
use eustress_common::services::sound::{
    parse_rolloff_mode, parse_sound_group, rolloff_mode_name, sound_group_name, write_sound_section, SoundPreview,
    ROLLOFF_MODES, SOUND_GROUPS,
};
use eustress_common::units::{convert_f32, Unit, ENGINE_NATIVE_UNIT};

use crate::space::file_loader::{FileType, LoadedFromFile};
use crate::space::instance_loader::InstanceFile;

/// Category the Sound rows appear under.
pub const SOUND_CATEGORY: &str = "Sound";

/// The panel's rows, in order: `(name, kind)`.
const ROWS: &[(&str, &str)] = &[
    ("Preview", "bool"),
    ("SoundId", "string"),
    ("Volume", "float"),
    ("PlaybackSpeed", "float"),
    ("Looped", "bool"),
    ("Playing", "bool"),
    ("TimePosition", "float"),
    ("SoundGroup", "choice"),
    ("RollOffMode", "choice"),
    ("RollOffMinDistance", "float"),
    ("RollOffMaxDistance", "float"),
];

/// The choices the RollOffMode row offers.
pub fn rolloff_mode_options() -> Vec<slint::SharedString> {
    ROLLOFF_MODES.iter().map(|m| slint::SharedString::from(*m)).collect()
}

/// The choices the SoundGroup row offers.
pub fn sound_group_options() -> Vec<slint::SharedString> {
    SOUND_GROUPS.iter().map(|g| slint::SharedString::from(*g)).collect()
}

/// A Sound's instance file: its `InstanceFile`, else, for a Sound in folder
/// form, the folder's `_instance.toml`. `None` for an audio file loaded as a
/// Sound (it has no file to save into).
pub fn sound_file(instance_toml: Option<&Path>, loaded: Option<&LoadedFromFile>) -> Option<PathBuf> {
    if let Some(p) = instance_toml {
        return Some(p.to_path_buf());
    }
    loaded
        .filter(|l| l.file_type == FileType::Directory)
        .map(|l| l.path.join("_instance.toml"))
}

fn fmt_f32(v: f32) -> String {
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".to_string() } else { s.to_string() }
}

fn parse_bool(text: &str) -> Result<bool, String> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        other => Err(format!("expected true or false, got {other:?}")),
    }
}

fn parse_non_negative(text: &str) -> Result<f32, String> {
    let v: f32 = text.trim().parse().map_err(|_| format!("expected a number, got {:?}", text.trim()))?;
    if !v.is_finite() {
        return Err("expected a finite number".into());
    }
    Ok(v.max(0.0))
}

/// Lengths are shown in the display unit and stored in metres.
fn is_length(key: &str) -> bool {
    matches!(key, "RollOffMinDistance" | "RollOffMaxDistance")
}

/// A saved field in the form the undo stack stores it (lengths in metres).
/// Preview is not a saved field.
pub fn get_text(sound: &Sound, key: &str) -> Option<String> {
    Some(match key {
        "SoundId" => sound.sound_id.clone(),
        "Volume" => fmt_f32(sound.volume),
        "PlaybackSpeed" => fmt_f32(sound.playback_speed),
        "Looped" => sound.looped.to_string(),
        "Playing" => sound.playing.to_string(),
        "TimePosition" => fmt_f32(sound.time_position),
        "SoundGroup" => sound_group_name(sound.sound_group).to_string(),
        "RollOffMode" => rolloff_mode_name(sound.roll_off_mode).to_string(),
        "RollOffMinDistance" => fmt_f32(sound.roll_off_min_distance),
        "RollOffMaxDistance" => fmt_f32(sound.roll_off_max_distance),
        _ => return None,
    })
}

/// Set a saved field from its stored text form.
pub fn set_text(sound: &mut Sound, key: &str, text: &str) -> Result<(), String> {
    match key {
        "SoundId" => sound.sound_id = text.trim().to_string(),
        "Volume" => sound.volume = parse_non_negative(text)?.min(10.0),
        "PlaybackSpeed" => {
            sound.playback_speed = parse_non_negative(text)?;
            sound.pitch = sound.playback_speed;
        }
        "Looped" => sound.looped = parse_bool(text)?,
        "Playing" => sound.playing = parse_bool(text)?,
        "TimePosition" => sound.time_position = parse_non_negative(text)?,
        "SoundGroup" => {
            sound.sound_group = parse_sound_group(text)
                .ok_or_else(|| format!("expected one of {}, got {:?}", SOUND_GROUPS.join(", "), text.trim()))?
        }
        "RollOffMode" => {
            sound.roll_off_mode = parse_rolloff_mode(text)
                .ok_or_else(|| format!("expected one of {}, got {:?}", ROLLOFF_MODES.join(", "), text.trim()))?
        }
        "RollOffMinDistance" => sound.roll_off_min_distance = parse_non_negative(text)?,
        "RollOffMaxDistance" => sound.roll_off_max_distance = parse_non_negative(text)?,
        _ => return Err(format!("Sound has no {key}")),
    }
    Ok(())
}

// ============================================================================
// Rows
// ============================================================================

/// Read access for building rows. The caller passes the display unit.
#[derive(SystemParam)]
pub struct PanelQueries<'w, 's> {
    sounds: Query<'w, 's, (&'static Sound, Has<SoundPreview>)>,
}

/// Property rows `(category, name, value, kind)` for a Sound, from its live
/// component, with lengths in the display `unit`. `None` for any other
/// class, or a Sound whose component has not been attached yet.
pub fn sound_rows(
    class: ClassName,
    entity: Entity,
    unit: Unit,
    queries: &PanelQueries,
) -> Option<Vec<(&'static str, &'static str, String, &'static str)>> {
    if class != ClassName::Sound {
        return None;
    }
    let (sound, previewing) = queries.sounds.get(entity).ok()?;
    Some(
        ROWS.iter()
            .filter_map(|(name, kind)| {
                let mut text = if *name == "Preview" { previewing.to_string() } else { get_text(sound, name)? };
                if is_length(name) && unit != ENGINE_NATIVE_UNIT {
                    if let Ok(m) = text.parse::<f32>() {
                        text = fmt_f32(convert_f32(m, ENGINE_NATIVE_UNIT, unit));
                    }
                }
                Some((SOUND_CATEGORY, *name, text, *kind))
            })
            .collect(),
    )
}

// ============================================================================
// Edits
// ============================================================================

/// Write access for panel edits. It holds no resources and no
/// `LoadedFromFile` (the drain system holds both mutably; a second access in
/// one system is a startup panic, B0002): the caller passes the display unit
/// and the Sound's file ([`sound_file`]).
#[derive(SystemParam)]
pub struct EditQueries<'w, 's> {
    commands: Commands<'w, 's>,
    sounds: Query<'w, 's, (&'static mut Sound, Has<SoundPreview>)>,
}

/// What a panel edit did, for the Output panel and the undo stack.
pub struct EditOutcome {
    pub message: String,
    pub undo: Option<crate::undo::Action>,
}

/// Apply a Properties edit if `key` is a field of the selected Sound. `None`
/// means "not ours" (Name, or any other class): the generic handler takes it.
/// `unit` is the display unit lengths are typed in; `toml_path` is the
/// Sound's file, `None` when it has none.
pub fn handle_edit(
    entity: Entity,
    key: &str,
    raw: &str,
    unit: Unit,
    toml_path: Option<&Path>,
    queries: &mut EditQueries,
) -> Option<Result<EditOutcome, String>> {
    if !ROWS.iter().any(|(name, _)| *name == key) {
        return None;
    }
    let (mut sound, previewing) = queries.sounds.get_mut(entity).ok()?;
    if key == "Preview" {
        return Some(match parse_bool(raw) {
            Ok(on) if on == previewing => Ok(EditOutcome { message: String::new(), undo: None }),
            Ok(true) => {
                queries.commands.entity(entity).insert(SoundPreview);
                Ok(EditOutcome { message: "Previewing the Sound".into(), undo: None })
            }
            Ok(false) => {
                queries.commands.entity(entity).remove::<SoundPreview>();
                Ok(EditOutcome { message: "Preview stopped".into(), undo: None })
            }
            Err(e) => Err(format!("Preview: {e}")),
        });
    }
    // Lengths arrive in the display unit; the component and file hold metres.
    let text = if is_length(key) && unit != ENGINE_NATIVE_UNIT {
        match raw.trim().parse::<f32>() {
            Ok(v) => fmt_f32(convert_f32(v, unit, ENGINE_NATIVE_UNIT)),
            Err(_) => return Some(Err(format!("{key}: expected a number, got {:?}", raw.trim()))),
        }
    } else {
        raw.to_string()
    };
    let old_text = get_text(&sound, key).unwrap_or_default();
    let mut edited = sound.clone();
    if let Err(e) = set_text(&mut edited, key, &text) {
        return Some(Err(format!("{key}: {e}")));
    }
    let new_text = get_text(&edited, key).unwrap_or_default();
    if new_text == old_text {
        return Some(Ok(EditOutcome { message: String::new(), undo: None }));
    }
    *sound = edited;
    let Some(path) = toml_path else {
        return Some(Ok(EditOutcome {
            message: format!("{key} = {new_text} (this Sound is an audio file, so the change is not saved)"),
            undo: None,
        }));
    };
    if let Err(e) = save_sound_section(path, &sound) {
        return Some(Err(format!("{key} changed but was not saved: {e}")));
    }
    Some(Ok(EditOutcome {
        message: format!("{key} = {new_text}"),
        undo: Some(crate::undo::Action::ChangeClassField {
            toml_path: path.to_path_buf(),
            property: key.to_string(),
            old_text,
            new_text,
        }),
    }))
}

/// Write a Sound's `[sound]` section into its instance file: the WorldDb
/// copy when a DB is active (authoritative on migrated Spaces) and the disk
/// mirror. Every other section, and keys of `[sound]` the Sound does not
/// own, are preserved.
pub fn save_sound_section(toml_path: &Path, sound: &Sound) -> Result<(), String> {
    let text = match crate::space::active_db::get_instance_text(toml_path) {
        Some(t) => t,
        None => std::fs::read_to_string(toml_path).map_err(|e| format!("read {}: {e}", toml_path.display()))?,
    };
    let mut doc: toml::Value = text.parse().map_err(|e| format!("parse {}: {e}", toml_path.display()))?;
    let Some(root) = doc.as_table_mut() else {
        return Err(format!("{} is not a TOML table", toml_path.display()));
    };
    let key = root.keys().find(|k| k.eq_ignore_ascii_case("sound")).cloned().unwrap_or_else(|| "sound".to_string());
    let section = root
        .entry(key)
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    if !section.is_table() {
        *section = toml::Value::Table(toml::value::Table::new());
    }
    if let Some(table) = section.as_table_mut() {
        write_sound_section(sound, table);
    }
    let out = toml::to_string_pretty(&doc).map_err(|e| format!("serialize {}: {e}", toml_path.display()))?;
    let db_ok = crate::space::active_db::put_instance_text(toml_path, &out);
    if let Err(e) = crate::space::gui_loader::write_atomic(toml_path, out.as_bytes()) {
        if !db_ok {
            return Err(format!("write {}: {e}", toml_path.display()));
        }
    }
    Ok(())
}

/// Set one field from its stored text form on the Sound whose file is
/// `toml_path`, and save the section: how the undo stack's
/// `ChangeClassField` replays a Sound edit. False when no Sound was loaded
/// from that file (the caller tries the other classes).
pub fn replay_field(world: &mut World, toml_path: &Path, property: &str, text: &str) -> bool {
    let mut sounds = world.query::<(Entity, &Sound, Option<&InstanceFile>, Option<&LoadedFromFile>)>();
    let Some(entity) = sounds
        .iter(world)
        .find(|(_, _, file, loaded)| {
            sound_file(file.map(|f| f.toml_path.as_path()), *loaded).as_deref() == Some(toml_path)
        })
        .map(|(e, ..)| e)
    else {
        return false;
    };
    let Some(mut sound) = world.get_mut::<Sound>(entity) else { return false };
    match set_text(&mut sound, property, text) {
        Ok(()) => {
            if let Err(e) = save_sound_section(toml_path, &sound) {
                warn!("undo {property}: {e}");
            }
        }
        Err(e) => warn!("undo {property} = {text}: {e}"),
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_saved_row_round_trips_through_its_text() {
        let mut sound = Sound::default();
        for (key, text) in [
            ("SoundId", "space://SoundService/flush.ogg"),
            ("Volume", "0.75"),
            ("PlaybackSpeed", "1.5"),
            ("Looped", "true"),
            ("Playing", "true"),
            ("TimePosition", "2.25"),
            ("SoundGroup", "Music"),
            ("RollOffMode", "Linear"),
            ("RollOffMinDistance", "4"),
            ("RollOffMaxDistance", "80"),
        ] {
            set_text(&mut sound, key, text).unwrap();
            assert_eq!(get_text(&sound, key).as_deref(), Some(text), "{key}");
        }
        assert_eq!(sound.pitch, 1.5, "PlaybackSpeed carries pitch with it");
    }

    #[test]
    fn bad_values_are_refused_or_clamped() {
        let mut sound = Sound::default();
        assert!(set_text(&mut sound, "RollOffMode", "Sideways").is_err());
        assert!(set_text(&mut sound, "SoundGroup", "Loud").is_err());
        assert!(set_text(&mut sound, "Looped", "maybe").is_err());
        assert!(set_text(&mut sound, "Preview", "true").is_err(), "Preview is not a saved field");
        set_text(&mut sound, "Volume", "40").unwrap();
        assert_eq!(sound.volume, 10.0);
        set_text(&mut sound, "PlaybackSpeed", "-2").unwrap();
        assert_eq!(sound.playback_speed, 0.0);
        set_text(&mut sound, "RollOffMode", "Enum.RollOffMode.InverseTapered").unwrap();
        assert_eq!(get_text(&sound, "RollOffMode").as_deref(), Some("Inverse"));
    }

    #[test]
    fn every_choice_offers_what_its_row_shows() {
        let sound = Sound::default();
        let modes = rolloff_mode_options();
        let groups = sound_group_options();
        assert!(modes.iter().any(|m| m.as_str() == get_text(&sound, "RollOffMode").unwrap()));
        assert!(groups.iter().any(|g| g.as_str() == get_text(&sound, "SoundGroup").unwrap()));
        let choices: Vec<&str> = ROWS.iter().filter(|(_, k)| *k == "choice").map(|(n, _)| *n).collect();
        assert_eq!(choices, ["SoundGroup", "RollOffMode"]);
    }

    #[test]
    fn a_folder_sound_saves_into_its_folders_instance_file() {
        let loaded = LoadedFromFile {
            path: PathBuf::from("S/Workspace/Door/Creak"),
            file_type: FileType::Directory,
            service: "Workspace".into(),
        };
        assert_eq!(
            sound_file(None, Some(&loaded)),
            Some(PathBuf::from("S/Workspace/Door/Creak").join("_instance.toml"))
        );
        let flat = PathBuf::from("S/Workspace/Creak.instance.toml");
        assert_eq!(sound_file(Some(&flat), Some(&loaded)), Some(flat.clone()));
        let audio = LoadedFromFile {
            path: PathBuf::from("S/SoundService/Gunshot.wav"),
            file_type: FileType::Wav,
            service: "SoundService".into(),
        };
        assert_eq!(sound_file(None, Some(&audio)), None);
    }

    #[test]
    fn saving_writes_the_section_and_keeps_the_rest() {
        let dir = std::env::temp_dir().join(format!("eustress_sound_panel_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("_instance.toml");
        std::fs::write(
            &path,
            "[metadata]\nclass_name = \"Sound\"\nname = \"Creak\"\n\n[sound]\nroll_off_mode = \"Inverse\"\nrolloff_mode = \"Inverse\"\nvolume = 0.5\n",
        )
        .unwrap();
        let mut sound = eustress_common::services::sound::sound_from_document(
            &std::fs::read_to_string(&path).unwrap().parse().unwrap(),
        );
        set_text(&mut sound, "RollOffMode", "Logarithmic").unwrap();
        save_sound_section(&path, &sound).unwrap();
        let doc: toml::Value = std::fs::read_to_string(&path).unwrap().parse().unwrap();
        assert_eq!(doc["metadata"]["name"].as_str(), Some("Creak"));
        assert_eq!(doc["sound"]["rolloff_mode"].as_str(), Some("Logarithmic"));
        assert!(doc["sound"].get("roll_off_mode").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
