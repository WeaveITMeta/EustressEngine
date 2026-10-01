//! # Sound Service
//! 
//! Audio playback and management.
//! 
//! Sound component matches engine/src/classes.rs::Sound for compatibility.
//! SoundService is the global audio settings resource.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// SoundService Resource
// ============================================================================

/// SoundService - global audio settings (like Eustress's SoundService)
#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct SoundService {
    /// Master volume (0-1)
    pub master_volume: f32,
    /// Ambient volume (0-1)
    pub ambient_volume: f32,
    /// Distance factor for 3D audio
    pub distance_factor: f32,
    /// Doppler scale
    pub doppler_scale: f32,
    /// Rolloff scale for distance attenuation
    pub rolloff_scale: f32,
    /// Is audio muted globally
    pub muted: bool,
    /// Respect distance for volume falloff
    pub respect_filtering_enabled: bool,
}

impl Default for SoundService {
    fn default() -> Self {
        Self {
            master_volume: 1.0,
            ambient_volume: 0.5,
            distance_factor: 1.0,
            doppler_scale: 1.0,
            rolloff_scale: 1.0,
            muted: false,
            respect_filtering_enabled: true,
        }
    }
}

// ============================================================================
// SoundInstance Component (runtime audio instance)
// ============================================================================

/// Runtime sound instance with playback state
/// Use classes::Sound for the base component, this for runtime state
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct SoundInstance {
    /// Audio asset (Eustress "SoundId")
    /// Bevy: Handle<AudioSource>
    pub sound_id: String,
    
    /// Volume (Eustress "Volume" 0-1)
    /// Bevy: PlaybackSettings.volume
    pub volume: f32,
    
    /// Pitch multiplier (Eustress "Pitch" 0.5-2)
    /// Bevy: PlaybackSettings.speed
    pub pitch: f32,
    
    /// Loop behavior (Eustress "Looped")
    /// Bevy: PlaybackSettings.looped
    pub looped: bool,
    
    /// Playback state (Eustress "Playing")
    /// Bevy: Play/pause
    #[serde(skip)]
    pub playing: bool,
    
    /// 3D spatial audio (Eustress implicit)
    pub spatial: bool,
    
    /// Max distance (Eustress "RollOffMaxDistance")
    pub roll_off_max_distance: f32,
    
    /// Min distance (Eustress "RollOffMinDistance")
    pub roll_off_min_distance: f32,
    
    /// Sound group for volume control
    pub sound_group: Option<String>,
    
    /// Time position in seconds (runtime)
    #[serde(skip)]
    pub time_position: f32,
    
    /// Total length in seconds (runtime)
    #[serde(skip)]
    pub time_length: f32,
}

impl Default for SoundInstance {
    fn default() -> Self {
        Self {
            sound_id: String::new(),
            volume: 0.5,
            pitch: 1.0,
            looped: false,
            playing: false,
            spatial: true,
            roll_off_max_distance: 10000.0,
            roll_off_min_distance: 10.0,
            sound_group: None,
            time_position: 0.0,
            time_length: 0.0,
        }
    }
}

impl SoundInstance {
    pub fn new(sound_id: impl Into<String>) -> Self {
        Self {
            sound_id: sound_id.into(),
            ..default()
        }
    }
    
    pub fn looped(mut self) -> Self {
        self.looped = true;
        self
    }
    
    pub fn volume(mut self, vol: f32) -> Self {
        self.volume = vol.clamp(0.0, 1.0);
        self
    }
    
    pub fn spatial(mut self, is_spatial: bool) -> Self {
        self.spatial = is_spatial;
        self
    }
}

// ============================================================================
// SoundGroup
// ============================================================================

/// SoundGroup - volume group for sounds (like Eustress's SoundGroup)
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct SoundGroup {
    /// Group name
    pub name: String,
    /// Group volume (0-1)
    pub volume: f32,
}

impl Default for SoundGroup {
    fn default() -> Self {
        Self {
            name: "Master".to_string(),
            volume: 1.0,
        }
    }
}

// ============================================================================
// Sound Events
// ============================================================================

/// Message to play a sound
#[derive(Message, Clone, Debug)]
pub struct PlaySoundEvent {
    pub entity: Entity,
}

/// Message to stop a sound
#[derive(Message, Clone, Debug)]
pub struct StopSoundEvent {
    pub entity: Entity,
}

/// Message to play a sound at a position (one-shot)
#[derive(Message, Clone, Debug)]
pub struct PlaySoundAtEvent {
    pub sound_id: String,
    pub position: Vec3,
    pub volume: f32,
}

// ============================================================================
// The Sound class's file section and its player
// ============================================================================
//
// A Sound instance file carries a `[sound]` section. Three writers have
// spelled its keys differently: the class schema (`playback_speed`,
// `rolloff_*`), the Roblox importer (both `rolloff_*` and `roll_off_*`, plus
// `pitch`), and the older spawner (`pitch`, `roll_off_*`). Keys are compared
// without case or underscores, so every spelling reads. Distances are metres
// (the importer converts studs); the file's `[metadata] unit` applies to its
// transform only.
//
// Studio and the Player both read Sounds through these functions, and the
// shared player (`eustress_play_runtime::sound_player`) plays them. The
// player never writes the authored `Sound`: what it reports goes into
// `SoundPlayback` and the two messages below.

use crate::classes::{Sound, SoundRolloffMode};

/// The RollOffMode choices, in the order Properties lists them.
pub const ROLLOFF_MODES: &[&str] = &["Inverse", "Linear", "InverseSquared", "Logarithmic", "None", "Custom"];

/// The SoundGroup choices, in the order Properties lists them.
pub const SOUND_GROUPS: &[&str] = &["Master", "SFX", "Music", "Voice", "Ambient", "UI"];

/// A RollOffMode by name, any case. Roblox's `InverseTapered` (its default)
/// reads as Inverse and its `LinearSquare` as Linear.
pub fn parse_rolloff_mode(text: &str) -> Option<SoundRolloffMode> {
    let t = text.trim();
    let t = t.rsplit('.').next().unwrap_or(t);
    Some(match t.to_ascii_lowercase().as_str() {
        "inverse" | "inversetapered" => SoundRolloffMode::Inverse,
        "linear" | "linearsquare" => SoundRolloffMode::Linear,
        "inversesquared" => SoundRolloffMode::InverseSquared,
        "logarithmic" => SoundRolloffMode::Logarithmic,
        "none" => SoundRolloffMode::None,
        "custom" => SoundRolloffMode::Custom,
        _ => return None,
    })
}

/// The name a RollOffMode is written and shown as.
pub fn rolloff_mode_name(mode: SoundRolloffMode) -> &'static str {
    match mode {
        SoundRolloffMode::Inverse => "Inverse",
        SoundRolloffMode::Linear => "Linear",
        SoundRolloffMode::InverseSquared => "InverseSquared",
        SoundRolloffMode::Logarithmic => "Logarithmic",
        SoundRolloffMode::None => "None",
        SoundRolloffMode::Custom => "Custom",
    }
}

/// A SoundGroup by name, any case.
pub fn parse_sound_group(text: &str) -> Option<crate::classes::SoundGroup> {
    use crate::classes::SoundGroup as G;
    let t = text.trim();
    let t = t.rsplit('.').next().unwrap_or(t);
    Some(match t.to_ascii_lowercase().as_str() {
        "master" => G::Master,
        "sfx" | "effects" => G::SFX,
        "music" => G::Music,
        "voice" => G::Voice,
        "ambient" => G::Ambient,
        "ui" => G::UI,
        _ => return None,
    })
}

/// The name a SoundGroup is written and shown as.
pub fn sound_group_name(group: crate::classes::SoundGroup) -> &'static str {
    use crate::classes::SoundGroup as G;
    match group {
        G::Master => "Master",
        G::SFX => "SFX",
        G::Music => "Music",
        G::Voice => "Voice",
        G::Ambient => "Ambient",
        G::UI => "UI",
    }
}

/// A key compared without case or underscores: `roll_off_mode`,
/// `rolloff_mode` and `RollOffMode` are one key.
fn key_form(key: &str) -> String {
    key.chars().filter(|c| *c != '_').flat_map(char::to_lowercase).collect()
}

fn get<'a>(section: &'a toml::Table, key: &str) -> Option<&'a toml::Value> {
    let want = key_form(key);
    section.iter().find(|(k, _)| key_form(k) == want).map(|(_, v)| v)
}

fn get_number(section: &toml::Table, key: &str) -> Option<f32> {
    let v = get(section, key)?;
    let n = v.as_float().or_else(|| v.as_integer().map(|i| i as f64))? as f32;
    n.is_finite().then_some(n)
}

/// Apply every field a `[sound]` section names onto `sound`; fields it
/// leaves out keep their value.
pub fn apply_sound_section(sound: &mut Sound, section: &toml::Table) {
    if let Some(id) = get(section, "sound_id").and_then(|v| v.as_str()) {
        sound.sound_id = id.to_string();
    }
    if let Some(v) = get_number(section, "volume") {
        sound.volume = v.clamp(0.0, 10.0);
    }
    if let Some(v) = get(section, "looped").and_then(|v| v.as_bool()) {
        sound.looped = v;
    }
    if let Some(v) = get(section, "playing").and_then(|v| v.as_bool()) {
        sound.playing = v;
    }
    // PlaybackSpeed, else the older `pitch`; both fields carry it.
    if let Some(v) = get_number(section, "playback_speed").or_else(|| get_number(section, "pitch")) {
        sound.playback_speed = v.max(0.0);
        sound.pitch = sound.playback_speed;
    }
    if let Some(v) = get_number(section, "time_position") {
        sound.time_position = v.max(0.0);
    }
    if let Some(v) = get_number(section, "rolloff_min_distance") {
        sound.roll_off_min_distance = v.max(0.0);
    }
    if let Some(v) = get_number(section, "rolloff_max_distance") {
        sound.roll_off_max_distance = v.max(0.0);
    }
    if let Some(m) = get(section, "rolloff_mode").and_then(|v| v.as_str()).and_then(parse_rolloff_mode) {
        sound.roll_off_mode = m;
    }
    if let Some(g) = get(section, "sound_group").and_then(|v| v.as_str()).and_then(parse_sound_group) {
        sound.sound_group = g;
    }
    if let Some(v) = get(section, "spatial").and_then(|v| v.as_bool()) {
        sound.spatial = v;
    }
}

/// A Sound from its `[sound]` section, class defaults for what it leaves out.
pub fn sound_from_section(section: &toml::Table) -> Sound {
    let mut sound = Sound::default();
    apply_sound_section(&mut sound, section);
    sound
}

/// A Sound from its whole instance file: the `[sound]` section, or the class
/// defaults when the file has none.
pub fn sound_from_document(doc: &toml::Value) -> Sound {
    doc.as_table()
        .and_then(|t| get(t, "sound"))
        .and_then(|v| v.as_table())
        .map(sound_from_section)
        .unwrap_or_default()
}

fn float_value(v: f32) -> toml::Value {
    // Through f32's shortest text, so 0.7 is written as 0.7.
    toml::Value::Float(v.to_string().parse::<f64>().unwrap_or(v as f64))
}

/// Write `sound` into a `[sound]` section in the class schema's spelling,
/// removing the other spellings of the same keys (and `pitch`, which mirrors
/// PlaybackSpeed) so one value is left for each. Keys it does not own stay.
pub fn write_sound_section(sound: &Sound, section: &mut toml::Table) {
    let owned: [(&str, toml::Value); 10] = [
        ("sound_id", toml::Value::String(sound.sound_id.clone())),
        ("volume", float_value(sound.volume)),
        ("looped", toml::Value::Boolean(sound.looped)),
        ("playing", toml::Value::Boolean(sound.playing)),
        ("playback_speed", float_value(sound.playback_speed)),
        ("time_position", float_value(sound.time_position)),
        ("rolloff_min_distance", float_value(sound.roll_off_min_distance)),
        ("rolloff_max_distance", float_value(sound.roll_off_max_distance)),
        ("rolloff_mode", toml::Value::String(rolloff_mode_name(sound.roll_off_mode).to_string())),
        ("sound_group", toml::Value::String(sound_group_name(sound.sound_group).to_string())),
    ];
    let had_spatial = get(section, "spatial").is_some();
    let forms: Vec<String> = owned
        .iter()
        .map(|(k, _)| key_form(k))
        .chain(["pitch".to_string(), "spatial".to_string()])
        .collect();
    section.retain(|k, _| !forms.contains(&key_form(k)));
    for (k, v) in owned {
        section.insert(k.to_string(), v);
    }
    if had_spatial || !sound.spatial {
        section.insert("spatial".to_string(), toml::Value::Boolean(sound.spatial));
    }
}

/// Roblox roll-off: the share of a Sound's volume heard `distance` metres
/// away. Full volume inside `min`, silent past `max` (except `None`, which
/// never fades), and in between by the mode. `Custom` follows `curve`
/// (distance as a share of `max`, volume), Inverse when it is empty.
pub fn rolloff_gain(mode: SoundRolloffMode, min: f32, max: f32, curve: &[(f32, f32)], distance: f32) -> f32 {
    if mode == SoundRolloffMode::None {
        return 1.0;
    }
    let min = if min.is_finite() { min.max(0.001) } else { 0.001 };
    let max = if max.is_finite() { max.max(min) } else { f32::MAX };
    let d = if distance.is_finite() { distance.max(0.0) } else { f32::MAX };
    if mode == SoundRolloffMode::Custom && !curve.is_empty() {
        return curve_at(curve, d / max).clamp(0.0, 1.0);
    }
    if d <= min {
        return 1.0;
    }
    if d >= max {
        return 0.0;
    }
    let g = match mode {
        SoundRolloffMode::Linear => (max - d) / (max - min),
        SoundRolloffMode::InverseSquared => (min / d) * (min / d),
        SoundRolloffMode::Logarithmic => 1.0 - (d / min).ln() / (max / min).ln(),
        _ => min / d,
    };
    g.clamp(0.0, 1.0)
}

/// A piecewise-linear curve of `(x, y)` points at `x`, held flat past its ends.
fn curve_at(curve: &[(f32, f32)], x: f32) -> f32 {
    let mut pts: Vec<(f32, f32)> = curve.iter().copied().filter(|(a, b)| a.is_finite() && b.is_finite()).collect();
    if pts.is_empty() {
        return 1.0;
    }
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    if x <= pts[0].0 {
        return pts[0].1;
    }
    for w in pts.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if x <= x1 {
            let span = x1 - x0;
            return if span <= f32::EPSILON { y1 } else { y0 + (y1 - y0) * (x - x0) / span };
        }
    }
    pts[pts.len() - 1].1
}

/// Play this Sound now, outside Play: the Properties panel's Preview. It
/// never starts by itself, and the player removes it when the clip ends or a
/// Play session starts.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SoundPreview;

/// A Sound entity a Play session made: the player plays it while a Play
/// session runs and its `playing` is true.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SoundInPlay;

/// Seek a playing Sound to this many seconds into its clip (a script setting
/// TimePosition). The player takes it and removes it.
#[derive(Component, Clone, Copy, Debug)]
pub struct SoundSeek(pub f32);

/// What the player reports about a Sound it plays. Only the player writes
/// it; the authored `Sound` is never changed by playback.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct SoundPlayback {
    /// Seconds into the clip.
    pub position: f32,
    /// The clip's length in seconds, when its format says.
    pub length: Option<f32>,
    /// The clip has loaded and a sink is playing it.
    pub loaded: bool,
    /// Sounding now (not paused).
    pub playing: bool,
}

/// A Sound that does not loop reached the end of its clip.
#[derive(Message, Clone, Copy, Debug)]
pub struct SoundEnded {
    pub entity: Entity,
}

/// A looping Sound went back to the start of its clip.
#[derive(Message, Clone, Copy, Debug)]
pub struct SoundLooped {
    pub entity: Entity,
}

/// The mix the Sound player applies on top of each Sound's own volume:
/// Master, and the Effects or Music slider by the Sound's group. The editor's
/// audio settings write it.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct SoundMix {
    pub master: f32,
    pub effects: f32,
    pub music: f32,
}

impl Default for SoundMix {
    fn default() -> Self {
        Self { master: 1.0, effects: 1.0, music: 1.0 }
    }
}

impl SoundMix {
    /// Master times the slider a group answers to (Music and Ambient to
    /// Music, Master to none, the rest to Effects).
    pub fn gain(&self, group: crate::classes::SoundGroup) -> f32 {
        use crate::classes::SoundGroup as G;
        let slider = match group {
            G::Music | G::Ambient => self.music,
            G::Master => 1.0,
            _ => self.effects,
        };
        (self.master * slider).max(0.0)
    }
}

#[cfg(test)]
mod sound_section_tests {
    use super::*;

    fn table(text: &str) -> toml::Table {
        text.parse::<toml::Table>().unwrap()
    }

    #[test]
    fn an_imported_section_reads_with_every_spelling() {
        // The Roblox importer's FlushSound, both roll-off spellings and pitch.
        let s = sound_from_section(&table(
            "looped = true\npitch = 1.5\nplayback_speed = 1.25\nplaying = false\n\
             roll_off_max_distance = 40.0\nroll_off_min_distance = 2.0\nroll_off_mode = \"Linear\"\n\
             rolloff_max_distance = 40.0\nrolloff_min_distance = 2.0\nrolloff_mode = \"Linear\"\n\
             sound_id = \"space://SoundService/flush.ogg\"\ntime_position = 0.0\nvolume = 1",
        ));
        assert!(s.looped);
        assert_eq!(s.playback_speed, 1.25, "PlaybackSpeed wins over pitch");
        assert_eq!(s.pitch, 1.25);
        assert_eq!(s.roll_off_min_distance, 2.0);
        assert_eq!(s.roll_off_max_distance, 40.0);
        assert_eq!(s.roll_off_mode, SoundRolloffMode::Linear);
        assert_eq!(s.volume, 1.0, "an integer volume reads");
        assert_eq!(s.sound_id, "space://SoundService/flush.ogg");
    }

    #[test]
    fn roblox_mode_names_read_and_unknown_ones_keep_the_default() {
        assert_eq!(parse_rolloff_mode("InverseTapered"), Some(SoundRolloffMode::Inverse));
        assert_eq!(parse_rolloff_mode("Enum.RollOffMode.LinearSquare"), Some(SoundRolloffMode::Linear));
        assert_eq!(parse_rolloff_mode("inversesquared"), Some(SoundRolloffMode::InverseSquared));
        assert_eq!(parse_rolloff_mode("sideways"), None);
        let s = sound_from_section(&table("rolloff_mode = \"sideways\""));
        assert_eq!(s.roll_off_mode, SoundRolloffMode::Inverse);
        for name in ROLLOFF_MODES {
            assert_eq!(rolloff_mode_name(parse_rolloff_mode(name).unwrap()), *name);
        }
        for name in SOUND_GROUPS {
            assert_eq!(sound_group_name(parse_sound_group(name).unwrap()), *name);
        }
    }

    #[test]
    fn writing_leaves_one_spelling_and_round_trips() {
        let mut section = table(
            "pitch = 1.0\nroll_off_mode = \"Inverse\"\nrolloff_mode = \"Inverse\"\n\
             roll_off_min_distance = 10.0\nextra_key = \"kept\"",
        );
        let mut s = Sound::default();
        s.volume = 0.7;
        s.playback_speed = 2.0;
        s.roll_off_mode = SoundRolloffMode::Logarithmic;
        s.sound_group = crate::classes::SoundGroup::Music;
        write_sound_section(&s, &mut section);
        assert!(section.get("pitch").is_none());
        assert!(section.get("roll_off_mode").is_none());
        assert!(section.get("roll_off_min_distance").is_none());
        assert_eq!(section.get("extra_key").and_then(|v| v.as_str()), Some("kept"));
        assert_eq!(section.get("volume").and_then(|v| v.as_float()), Some(0.7));
        assert!(section.get("spatial").is_none(), "spatial is written only when it was there or is off");
        let back = sound_from_section(&section);
        assert_eq!(back.volume, 0.7);
        assert_eq!(back.playback_speed, 2.0);
        assert_eq!(back.roll_off_mode, SoundRolloffMode::Logarithmic);
        assert_eq!(back.sound_group, crate::classes::SoundGroup::Music);
    }

    #[test]
    fn a_file_with_no_sound_section_reads_the_class_defaults() {
        let doc: toml::Value = "[metadata]\nclass_name = \"Sound\"".parse().unwrap();
        let s = sound_from_document(&doc);
        assert_eq!(s.volume, Sound::default().volume);
        let doc: toml::Value = "[sound]\nvolume = 0.25".parse().unwrap();
        assert_eq!(sound_from_document(&doc).volume, 0.25);
    }

    #[test]
    fn rolloff_follows_roblox() {
        use SoundRolloffMode as M;
        for mode in [M::Inverse, M::Linear, M::InverseSquared, M::Logarithmic] {
            assert_eq!(rolloff_gain(mode, 10.0, 100.0, &[], 5.0), 1.0, "{mode:?} is full inside min");
            assert_eq!(rolloff_gain(mode, 10.0, 100.0, &[], 100.0), 0.0, "{mode:?} is silent at max");
            let near = rolloff_gain(mode, 10.0, 100.0, &[], 20.0);
            let far = rolloff_gain(mode, 10.0, 100.0, &[], 60.0);
            assert!(near > far && far > 0.0, "{mode:?} fades: {near} then {far}");
        }
        assert!((rolloff_gain(M::Inverse, 10.0, 100.0, &[], 20.0) - 0.5).abs() < 1e-6);
        assert!((rolloff_gain(M::InverseSquared, 10.0, 100.0, &[], 20.0) - 0.25).abs() < 1e-6);
        assert!((rolloff_gain(M::Linear, 10.0, 100.0, &[], 55.0) - 0.5).abs() < 1e-6);
        assert_eq!(rolloff_gain(M::None, 10.0, 100.0, &[], 1.0e6), 1.0);
        let curve = [(0.0, 1.0), (0.5, 0.5), (1.0, 0.0)];
        assert!((rolloff_gain(M::Custom, 10.0, 100.0, &curve, 25.0) - 0.75).abs() < 1e-6);
        assert!((rolloff_gain(M::Custom, 10.0, 100.0, &[], 20.0) - 0.5).abs() < 1e-6);
        assert_eq!(rolloff_gain(M::Inverse, 0.0, 0.0, &[], f32::NAN), 0.0);
    }

    #[test]
    fn the_mix_follows_the_group() {
        let mix = SoundMix { master: 0.5, effects: 0.4, music: 0.8 };
        assert!((mix.gain(crate::classes::SoundGroup::Music) - 0.4).abs() < 1e-6);
        assert!((mix.gain(crate::classes::SoundGroup::SFX) - 0.2).abs() < 1e-6);
        assert!((mix.gain(crate::classes::SoundGroup::Master) - 0.5).abs() < 1e-6);
    }
}
