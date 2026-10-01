//! Play's side of the Sound player ([`crate::sound_player`]): which Sounds it
//! plays in a session, and what it reports back into the tree.
//!
//! Every Sound in a session's tree plays through the player. The Play step
//! spawns an entity for each Sound a script makes or a host replicates
//! (`apply`), already marked [`SoundInPlay`]. Studio binds the Space's own
//! Sounds to their edit entities instead of spawning them, so
//! [`mark_play_sounds`] marks those while the session runs and unmarks them
//! when it ends.
//!
//! [`pull_sounds`] runs in `PlayScriptSet::Pull`, so scripts read this frame's
//! state:
//! - `TimePosition` (quietly: Roblox never announces it as it plays),
//!   `TimeLength` and `IsLoaded`, as the engine's writes, which are never a
//!   seek and never replicate;
//! - `Ended`, `DidLoop` (with the loops since the last `Play()`) and `Loaded`
//!   as signals with the `SoundId`, which the VM fires at its next resumption
//!   point;
//! - a clip that ended stops, in the tree and on its component alike, so the
//!   two never disagree about whether it plays.

use std::collections::HashSet;

use bevy::prelude::*;

use eustress_common::classes::Sound;
use eustress_common::datamodel::{DataModel, DmEvent, DmValue, InstanceId, SOUND_LOOPS};
use eustress_common::play_session::{DataModelSpawned, PlayDataModel, PlayScriptSet};
use eustress_common::services::sound::{SoundEnded, SoundInPlay, SoundLooped, SoundPlayback, SoundSeek};

use crate::sound_player::SoundPlayerSystems;

/// The session's Sound systems, in both apps: the pull in `Pull`, the
/// marking before the player, and the player after `Apply`.
pub struct SoundBridgePlugin;

impl Plugin for SoundBridgePlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(Update, SoundPlayerSystems.after(PlayScriptSet::Apply)).add_systems(
            Update,
            (pull_sounds.in_set(PlayScriptSet::Pull), mark_play_sounds.before(SoundPlayerSystems)),
        );
    }
}

/// A bound Sound's component as it was when the session began. Studio's
/// Stop restores its parts, not its Sounds, and scripts and the pull write
/// the component during Play, so it is put back from here.
#[derive(Component, Debug, Clone)]
pub struct SoundBeforePlay(pub Sound);

/// Marks the Sounds a session's tree binds to entities it did not spawn
/// (Studio's Space Sounds) as the session's while it runs, and unmarks them
/// when it ends. Spawned ones are marked where they are made and go with the
/// session.
pub fn mark_play_sounds(
    mut commands: Commands,
    dm: Option<Res<PlayDataModel>>,
    unmarked: Query<(Entity, &Sound), Without<SoundInPlay>>,
    marked: Query<(Entity, Option<&SoundBeforePlay>), (With<SoundInPlay>, Without<DataModelSpawned>)>,
) {
    match dm {
        Some(dm) => {
            if unmarked.is_empty() {
                return;
            }
            let g = dm.dm.lock();
            for (entity, sound) in &unmarked {
                if g.by_entity(entity.to_bits()).is_some() {
                    commands.entity(entity).insert((SoundInPlay, SoundBeforePlay(sound.clone())));
                }
            }
        }
        None => {
            for (entity, before) in &marked {
                let mut e = commands.entity(entity);
                e.remove::<(SoundInPlay, SoundBeforePlay, SoundSeek)>();
                if let Some(before) = before {
                    e.insert(before.0.clone());
                }
            }
        }
    }
}

fn sound_id(g: &DataModel, id: InstanceId) -> DmValue {
    g.get_prop(id, "SoundId").unwrap_or_else(|| DmValue::String(String::new()))
}

/// The player's reports into the tree (see the module docs).
pub fn pull_sounds(
    dm: Option<Res<PlayDataModel>>,
    mut sounds: Query<(Entity, &SoundPlayback, &mut Sound), With<SoundInPlay>>,
    mut ended: MessageReader<SoundEnded>,
    mut looped: MessageReader<SoundLooped>,
    mut loaded: Local<HashSet<Entity>>,
) {
    let Some(dm) = dm else {
        ended.clear();
        looped.clear();
        loaded.clear();
        return;
    };
    let mut g = dm.dm.lock();
    // `Loaded` fires once per Sound, however often its sink restarts.
    loaded.retain(|e| sounds.contains(*e));
    for (entity, report, _) in &sounds {
        let Some(id) = g.by_entity(entity.to_bits()) else { continue };
        g.set_prop_quietly(id, "TimePosition", DmValue::Number(report.position as f64));
        g.set_prop_from_engine(id, "TimeLength", DmValue::Number(report.length.unwrap_or(0.0) as f64));
        g.set_prop_from_engine(id, "IsLoaded", DmValue::Bool(report.loaded));
        if report.loaded && loaded.insert(entity) {
            let args = vec![sound_id(&g, id)];
            g.push_event(DmEvent::Signal { id, name: "Loaded".into(), args });
        }
    }
    for e in looped.read() {
        let Some(id) = g.by_entity(e.entity.to_bits()) else { continue };
        let n = g.get_prop(id, SOUND_LOOPS).and_then(|v| v.as_number()).unwrap_or(0.0) + 1.0;
        g.set_prop_from_engine(id, SOUND_LOOPS, DmValue::Number(n));
        let args = vec![sound_id(&g, id), DmValue::Number(n)];
        g.push_event(DmEvent::Signal { id, name: "DidLoop".into(), args });
    }
    for e in ended.read() {
        let Some(id) = g.by_entity(e.entity.to_bits()) else { continue };
        g.set_prop_from_engine(id, "Playing", DmValue::Bool(false));
        g.set_prop_quietly(id, "TimePosition", DmValue::Number(0.0));
        let args = vec![sound_id(&g, id)];
        g.push_event(DmEvent::Signal { id, name: "Ended".into(), args });
        if let Ok((_, _, mut s)) = sounds.get_mut(e.entity) {
            s.playing = false;
            s.time_position = 0.0;
        }
    }
}
