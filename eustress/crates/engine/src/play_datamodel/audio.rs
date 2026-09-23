//! `Sound:Play()`, `:Stop()`, `:Pause()`, `:Resume()` in Play.
//!
//! `SoundId` is a path inside the Space (`Assets/Sounds/shot.ogg`) or any
//! Bevy asset URL (`space://…`). A Sound parented to a part fades with the
//! listener's distance over `RollOffMaxDistance`; one parented elsewhere
//! (SoundService, a GUI) plays at its `Volume` everywhere.

use std::collections::{HashMap, HashSet};

use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackSettings, Volume};
use bevy::prelude::*;

use eustress_common::avatar::LocalAvatar;
use eustress_common::datamodel::{is_base_part, InstanceId, SoundAction};

use super::{DataModelSpawned, PlayDataModel};

/// A Sound's `SoundId` as an asset URL, or `None` for ids Eustress cannot
/// play (Roblox asset ids).
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

/// Past this many tracked players, entries for Sounds that no longer exist
/// are dropped (a game clones a Sound per shot and destroys it after).
const PRUNE_AT: usize = 256;

#[allow(clippy::too_many_arguments)]
pub fn apply_sound_commands(
    mut commands: Commands,
    dm: Option<Res<PlayDataModel>>,
    asset_server: Res<AssetServer>,
    mut playing: Local<HashMap<InstanceId, Entity>>,
    // One strong handle per file for the session, so a sound played again
    // after its last player finished is not read and decoded again.
    mut sources: Local<HashMap<String, Handle<AudioSource>>>,
    sinks: Query<&AudioSink>,
    listener: Query<&GlobalTransform, With<LocalAvatar>>,
    mut warned: Local<HashSet<String>>,
    mut session: Local<usize>,
) {
    let Some(dm) = dm else {
        playing.clear();
        sources.clear();
        return;
    };
    // Locals outlive a session (this runs only while Playing); instance ids
    // restart with each tree, so an old entry would answer for a new Sound.
    let tree = std::sync::Arc::as_ptr(&dm.dm) as usize;
    if *session != tree {
        *session = tree;
        playing.clear();
        sources.clear();
    }
    let cmds = std::mem::take(&mut dm.dm.lock().sound_commands);
    if cmds.is_empty() {
        return;
    }
    if playing.len() > PRUNE_AT {
        let g = dm.dm.lock();
        playing.retain(|id, _| g.exists(*id));
    }
    let ear = listener.iter().next().map(|g| g.translation());
    for cmd in cmds {
        match cmd.action {
            SoundAction::Play => {
                let (url, volume, speed, looped, pos, rolloff) = {
                    let g = dm.dm.lock();
                    let id = g.get_prop(cmd.sound, "SoundId").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                    let volume = g.get_prop(cmd.sound, "Volume").and_then(|v| v.as_number()).unwrap_or(0.5) as f32;
                    let speed = g.get_prop(cmd.sound, "PlaybackSpeed").and_then(|v| v.as_number()).unwrap_or(1.0) as f32;
                    let looped = g.get_prop(cmd.sound, "Looped").and_then(|v| v.as_bool()).unwrap_or(false);
                    let rolloff = g.get_prop(cmd.sound, "RollOffMaxDistance").and_then(|v| v.as_number()).unwrap_or(60.0) as f32;
                    let pos = g
                        .parent(cmd.sound)
                        .filter(|p| g.get(*p).map_or(false, |i| is_base_part(&i.class_name)))
                        .and_then(|p| g.get(p).and_then(|i| i.cframe()))
                        .map(|cf| cf.position.to_vec3());
                    (sound_url(&id), volume, speed, looped, pos, rolloff)
                };
                let Some(url) = url else {
                    let id = dm.dm.lock().get_prop(cmd.sound, "SoundId").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                    if warned.insert(id.clone()) {
                        dm.dm.lock().print(
                            eustress_common::datamodel::OutputLevel::Warn,
                            "Sound",
                            format!("SoundId '{}' is not a Space audio file; put the file in the Space and use its path", id),
                        );
                    }
                    continue;
                };
                // Distance fade for sounds in the world.
                let gain = match (pos, ear) {
                    (Some(p), Some(e)) => (1.0 - p.distance(e) / rolloff.max(0.1)).clamp(0.0, 1.0),
                    _ => 1.0,
                };
                if gain <= 0.001 {
                    continue;
                }
                if let Some(prev) = playing.remove(&cmd.sound) {
                    if let Ok(mut ec) = commands.get_entity(prev) {
                        ec.despawn();
                    }
                }
                let settings = if looped { PlaybackSettings::LOOP } else { PlaybackSettings::DESPAWN }
                    .with_volume(Volume::Linear((volume * gain).max(0.0)))
                    .with_speed(speed.max(0.01));
                let source = sources.entry(url).or_insert_with_key(|url| asset_server.load(url.clone())).clone();
                let e = commands.spawn((AudioPlayer::<AudioSource>(source), settings, DataModelSpawned)).id();
                playing.insert(cmd.sound, e);
            }
            SoundAction::Stop => {
                if let Some(prev) = playing.remove(&cmd.sound) {
                    if let Ok(mut ec) = commands.get_entity(prev) {
                        ec.despawn();
                    }
                }
            }
            SoundAction::Pause => {
                if let Some(sink) = playing.get(&cmd.sound).and_then(|e| sinks.get(*e).ok()) {
                    sink.pause();
                }
            }
            SoundAction::Resume => {
                if let Some(sink) = playing.get(&cmd.sound).and_then(|e| sinks.get(*e).ok()) {
                    sink.play();
                }
            }
        }
    }
}
