//! The Sound player: plays a `Sound` component on its own entity, in Studio
//! and the Player alike.
//!
//! What plays:
//! - a Sound marked [`SoundPreview`] while no Play session runs (the
//!   Properties panel's Preview; a Sound never starts by itself in Edit);
//! - a Sound marked [`SoundInPlay`] (an entity a Play session made) while a
//!   session runs and its `playing` is true. `playing` false pauses it where
//!   it is.
//!
//! When a session starts every preview stops; when it ends every Play sound
//! stops. The sink is on the Sound entity itself, so a despawned Sound (a
//! per-shot clone) takes its sink with it, and a dropped sink stops.
//!
//! A running sink follows the component live: Volume, PlaybackSpeed,
//! SoundGroup and the roll-off every frame; Looped and SoundId by restarting
//! (Bevy fixes a sink's mode when it starts), Looped where the clip was. The
//! volume is the Sound's Volume times the Roblox roll-off from the listener
//! (`rolloff_gain`, for a Sound in a part or an Attachment; one anywhere else
//! is heard everywhere) times the editor's mix. Bevy's spatial curve is not
//! used. The listener is the active view camera that draws to a window.
//!
//! The player never writes the authored `Sound`. It reports in
//! [`SoundPlayback`] and sends [`SoundEnded`] and [`SoundLooped`]; a script's
//! seek arrives as [`SoundSeek`]. A clip starts paused, seeks to its start
//! (the Sound's TimePosition, or the seek) and then plays, so the sink's
//! position is always seconds into the clip.

use std::collections::HashSet;
use std::time::Duration;

use bevy::audio::{
    AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, Decodable, PlaybackMode, PlaybackSettings, Source, Volume,
};
use bevy::camera::RenderTarget;
use bevy::prelude::*;

use eustress_common::classes::{ClassName, Instance, Sound};
use eustress_common::play_session::PlayDataModel;
use eustress_common::plugins::sky_atmosphere::SkyCamera;
use eustress_common::services::sound::{
    rolloff_gain, SoundEnded, SoundInPlay, SoundLooped, SoundMix, SoundPlayback, SoundPreview, SoundSeek,
};

/// A Sound's `SoundId` as an asset URL, or `None` for ids Eustress cannot
/// play (Roblox asset ids). A path inside the Space (`Assets/Sounds/shot.ogg`)
/// becomes `space://...`, which a Player resolves in its own copy of the Space.
pub fn sound_url(id: &str) -> Option<String> {
    let id = id.trim().replace('\\', "/");
    if id.is_empty() || id.starts_with("rbxassetid://") || id.starts_with("rbxasset://") {
        return None;
    }
    if id.contains("://") {
        return Some(id);
    }
    Some(format!("space://{}", id.trim_start_matches('/')))
}

/// The player's step. A Play app orders it after the session's apply
/// ([`crate::sound_bridge::SoundBridgePlugin`]), so a script's `Play()` sounds
/// in the same frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SoundPlayerSystems;

/// Plays Sound components. Studio mounts it (build 14); the shared Play
/// runtime takes the mount over when Play Sounds become entities.
pub struct SoundPlayerPlugin;

impl Plugin for SoundPlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SoundMix>()
            .add_message::<SoundEnded>()
            .add_message::<SoundLooped>()
            .add_systems(Update, drive_sounds.in_set(SoundPlayerSystems));
    }
}

/// What the running sink was started with, and what the player has applied
/// to it since.
#[derive(Component, Debug, Clone)]
pub struct SoundSink {
    url: String,
    looped: bool,
    /// Seconds into the clip to seek to once the sink exists.
    pending_seek: Option<f32>,
    applied_gain: f32,
    applied_speed: f32,
    /// Loops counted so far, from the sink's position over the clip length.
    wraps: u64,
    last_position: f32,
    length: Option<f32>,
    ended: bool,
}

impl SoundSink {
    fn new(url: String, looped: bool, start: f32) -> Self {
        Self {
            url,
            looped,
            pending_seek: Some(start.max(0.0)),
            applied_gain: f32::NAN,
            applied_speed: f32::NAN,
            wraps: 0,
            last_position: 0.0,
            length: None,
            ended: false,
        }
    }
}

/// Whether the player owns a Sound this frame.
pub fn owned(in_play: bool, preview: bool, play_entity: bool) -> bool {
    if in_play { play_entity } else { preview }
}

/// The components that start a Sound, all on the Sound's own entity. The
/// clip starts paused; the player seeks it to `start` and then plays it.
pub fn start_bundle(source: Handle<AudioSource>, url: String, looped: bool, start: f32) -> (AudioPlayer, PlaybackSettings, SoundSink) {
    let settings = PlaybackSettings {
        mode: if looped { PlaybackMode::Loop } else { PlaybackMode::Once },
        // Set on the first frame the sink exists, with the mix and roll-off.
        volume: Volume::Linear(0.0),
        speed: 1.0,
        paused: true,
        muted: false,
        spatial: false,
        spatial_scale: None,
        start_position: None,
        duration: None,
    };
    (AudioPlayer::<AudioSource>(source), settings, SoundSink::new(url, looped, start))
}

/// A Sound's volume at the listener: its Volume, the roll-off over the
/// distance when it is placed in the world (`at`) and there is a listener
/// (`ear`), and the mix for its group.
pub fn sound_gain(sound: &Sound, at: Option<Vec3>, ear: Option<Vec3>, mix: &SoundMix) -> f32 {
    let rolloff = match (at, ear) {
        (Some(a), Some(e)) => rolloff_gain(
            sound.roll_off_mode,
            sound.roll_off_min_distance,
            sound.roll_off_max_distance,
            &sound.roll_off_curve,
            a.distance(e),
        ),
        _ => 1.0,
    };
    (sound.volume.max(0.0) * rolloff * mix.gain(sound.sound_group)).max(0.0)
}

/// A Sound is heard from where it is when its parent is a part or an
/// Attachment, as in Roblox; anywhere else it is heard everywhere.
fn is_placed(parent_class: Option<ClassName>) -> bool {
    parent_class.is_some_and(|c| c == ClassName::Attachment || eustress_common::datamodel::is_base_part(c.as_str()))
}

#[allow(clippy::type_complexity)]
fn drive_sounds(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    sources: Res<Assets<AudioSource>>,
    mix: Res<SoundMix>,
    play: Option<Res<PlayDataModel>>,
    cameras: Query<(&Camera, &GlobalTransform, Option<&RenderTarget>), With<SkyCamera>>,
    instances: Query<&Instance>,
    mut sounds: Query<(
        Entity,
        &Sound,
        Option<&GlobalTransform>,
        Option<&ChildOf>,
        Has<SoundPreview>,
        Has<SoundInPlay>,
        Option<&SoundSeek>,
        Option<&mut SoundSink>,
        Option<&mut AudioSink>,
        Option<&AudioPlayer>,
        Option<&mut SoundPlayback>,
    )>,
    mut ended: MessageWriter<SoundEnded>,
    mut looped: MessageWriter<SoundLooped>,
    mut warned: Local<HashSet<String>>,
) {
    let in_play = play.is_some();
    // The ears: the active view camera drawing to a window, the highest order.
    let ear = cameras
        .iter()
        .filter(|(cam, _, target)| cam.is_active && matches!(target, None | Some(RenderTarget::Window(_))))
        .max_by_key(|(cam, _, _)| cam.order)
        .map(|(_, t, _)| t.translation());

    for (entity, sound, at, parent, preview, play_entity, seek, state, sink, player, report) in &mut sounds {
        // A session started: previews stop and do not come back after Stop.
        if in_play && preview {
            commands.entity(entity).remove::<SoundPreview>();
        }
        let url = owned(in_play, preview, play_entity).then(|| sound_url(&sound.sound_id)).flatten();
        let Some(url) = url else {
            if state.is_some() {
                commands.entity(entity).remove::<(AudioPlayer, AudioSink, PlaybackSettings, SoundSink)>();
            }
            if let Some(mut report) = report {
                report.set_if_neq(SoundPlayback::default());
            }
            let id = sound.sound_id.trim();
            if owned(in_play, preview, play_entity) && !id.is_empty() && warned.insert(id.to_string()) {
                warn!("Sound: SoundId '{id}' is not a Space audio file; put the file in the Space and use its path");
            }
            continue;
        };
        let want_playing = if in_play { sound.playing } else { true };

        // (Re)start: nothing running yet, a new clip, a changed Looped, or a
        // seek on a clip that has ended. An ended Play sound waits for the
        // tree to reset it (or seek it); it never restarts itself.
        let restart_at = match &state {
            None => want_playing.then_some(seek.map_or(sound.time_position, |s| s.0)),
            Some(s) if s.url != url => Some(seek.map_or(0.0, |k| k.0)),
            Some(s) if s.looped != sound.looped => Some(seek.map_or(s.last_position, |k| k.0)),
            Some(s) if s.ended => seek.filter(|_| want_playing).map(|k| k.0),
            Some(_) => None,
        };
        if let Some(start) = restart_at {
            let handle: Handle<AudioSource> = match player.filter(|_| state.as_ref().is_some_and(|s| s.url == url)) {
                Some(p) => p.0.clone(),
                None => asset_server.load(url.clone()),
            };
            commands
                .entity(entity)
                .remove::<AudioSink>()
                .remove::<SoundSeek>()
                .insert(start_bundle(handle, url, sound.looped, start));
            let loading = SoundPlayback { position: start.max(0.0), length: None, loaded: false, playing: false };
            match report {
                Some(mut r) => {
                    r.set_if_neq(loading);
                }
                None => {
                    commands.entity(entity).insert(loading);
                }
            }
            continue;
        }
        let (Some(mut state), Some(mut sink)) = (state, sink) else {
            // Loading, or a paused Play sound not yet started.
            continue;
        };

        // A seek on a live sink continues from there.
        if let Some(k) = seek {
            state.pending_seek = Some(k.0.max(0.0));
            commands.entity(entity).remove::<SoundSeek>();
        }
        if let Some(to) = state.pending_seek.take() {
            if to > 0.0 {
                if let Err(e) = sink.try_seek(Duration::from_secs_f32(to)) {
                    if warned.insert(format!("seek:{}", state.url)) {
                        warn!("Sound: {} cannot seek ({e:?}); it plays from the start", state.url);
                    }
                }
            }
            state.last_position = to;
            state.wraps = 0;
        }
        if want_playing && sink.is_paused() {
            sink.play();
        } else if !want_playing && !sink.is_paused() {
            sink.pause();
        }

        let speed = sound.playback_speed.max(0.01);
        if (speed - state.applied_speed).abs() > 1e-4 || state.applied_speed.is_nan() {
            sink.set_speed(speed);
            state.applied_speed = speed;
        }
        let placed = is_placed(parent.and_then(|p| instances.get(p.parent()).ok()).map(|i| i.class_name));
        let gain = sound_gain(sound, if placed { at.map(GlobalTransform::translation) } else { None }, ear, &mix);
        if (gain - state.applied_gain).abs() > 1e-4 || state.applied_gain.is_nan() {
            sink.set_volume(Volume::Linear(gain));
            state.applied_gain = gain;
        }

        // Where the clip is, how long it is, and its ends.
        if state.length.is_none() {
            if let Some(src) = player.and_then(|p| sources.get(&p.0)) {
                state.length = src.decoder().total_duration().map(|d| d.as_secs_f32()).filter(|l| *l > 0.0);
            }
        }
        let raw = sink.position().as_secs_f32();
        let position = match state.length {
            Some(len) if state.looped => {
                let wraps = (raw / len).floor().max(0.0) as u64;
                if wraps > state.wraps {
                    state.wraps = wraps;
                    looped.write(SoundLooped { entity });
                }
                raw % len
            }
            Some(len) => raw.min(len),
            None => raw,
        };
        if state.looped && state.length.is_none() && raw + 0.05 < state.last_position {
            looped.write(SoundLooped { entity });
        }
        state.last_position = position;
        if !state.looped && !state.ended && sink.empty() {
            state.ended = true;
            ended.write(SoundEnded { entity });
            if preview {
                commands.entity(entity).remove::<SoundPreview>();
            }
        }
        let now = SoundPlayback {
            position,
            length: state.length,
            loaded: true,
            playing: want_playing && !state.ended && !sink.is_paused(),
        };
        match report {
            Some(mut r) => {
                r.set_if_neq(now);
            }
            None => {
                commands.entity(entity).insert(now);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_ids_resolve_by_sound_urls_rule() {
        assert_eq!(sound_url("Assets/Sounds/shot.ogg").as_deref(), Some("space://Assets/Sounds/shot.ogg"));
        assert_eq!(sound_url("/SoundService/Gunshot.wav").as_deref(), Some("space://SoundService/Gunshot.wav"));
        assert_eq!(sound_url("SoundService\\Gunshot.wav").as_deref(), Some("space://SoundService/Gunshot.wav"));
        assert_eq!(sound_url("space://SoundService/flush.ogg").as_deref(), Some("space://SoundService/flush.ogg"));
        assert_eq!(sound_url("bundled://sounds/click.ogg").as_deref(), Some("bundled://sounds/click.ogg"));
        assert_eq!(sound_url("rbxassetid://367735596"), None);
        assert_eq!(sound_url("rbxasset://sounds/x.wav"), None);
        assert_eq!(sound_url("   "), None);
    }

    #[test]
    fn the_volume_follows_roblox_rolloff_and_the_mix() {
        let sound = Sound {
            volume: 0.8,
            roll_off_min_distance: 10.0,
            roll_off_max_distance: 100.0,
            ..Default::default()
        };
        let mix = SoundMix { master: 0.5, effects: 1.0, music: 1.0 };
        let at = Some(Vec3::ZERO);
        // Inverse (the default): half volume at twice the min distance.
        let g = sound_gain(&sound, at, Some(Vec3::new(20.0, 0.0, 0.0)), &mix);
        assert!((g - 0.8 * 0.5 * 0.5).abs() < 1e-6, "{g}");
        // Inside min: full; past max: silent.
        assert!((sound_gain(&sound, at, Some(Vec3::X * 5.0), &mix) - 0.4).abs() < 1e-6);
        assert_eq!(sound_gain(&sound, at, Some(Vec3::X * 150.0), &mix), 0.0);
        // Not placed in the world, or no listener: heard everywhere.
        assert!((sound_gain(&sound, None, Some(Vec3::X * 150.0), &mix) - 0.4).abs() < 1e-6);
        assert!((sound_gain(&sound, at, None, &mix) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn only_a_part_or_an_attachment_places_a_sound() {
        assert!(is_placed(Some(ClassName::Part)));
        assert!(is_placed(Some(ClassName::Attachment)));
        assert!(!is_placed(Some(ClassName::Folder)));
        assert!(!is_placed(None));
    }

    #[test]
    fn edit_plays_previews_and_play_plays_play_sounds() {
        assert!(owned(false, true, false));
        assert!(!owned(false, false, true), "a Play sound never sounds in Edit");
        assert!(owned(true, false, true));
        assert!(!owned(true, true, false), "a preview stops when a session starts");
    }

    #[test]
    fn the_sink_lives_on_the_sound_and_goes_with_it() {
        let mut world = World::new();
        let sound = world.spawn(Sound::default()).id();
        world
            .entity_mut(sound)
            .insert(start_bundle(Handle::default(), "space://a.ogg".into(), false, 1.5));
        let mut players = world.query::<(Entity, &AudioPlayer, &PlaybackSettings, &SoundSink)>();
        let (holder, _, settings, state) = players.single(&world).unwrap();
        assert_eq!(holder, sound, "no side entity holds the clip");
        assert!(settings.paused, "it starts paused and plays after its seek");
        assert!(!settings.spatial, "the roll-off is ours, not Bevy's");
        assert_eq!(state.pending_seek, Some(1.5));
        world.despawn(sound);
        assert_eq!(world.query::<&AudioPlayer>().iter(&world).count(), 0);
        assert_eq!(world.query::<&SoundSink>().iter(&world).count(), 0);
    }
}
