//! Terrain edits against the live world: the entry point the MCP terrain
//! tools, the Luau `workspace.Terrain` methods and the Rune
//! `eustress::terrain` module share.
//!
//! [`apply_terrain_commands`] applies a batch of [`TerrainCommand`]s to the
//! Space's one `TerrainRoot`, in order, through
//! `eustress_common::terrain::api::apply_terrain_command`, and marks what each
//! changed in `TerrainDirtyChunks`, which remeshes and re-collides those
//! chunks. The commands edit the root's base data; a layer bake re-bakes over
//! them. [`with_terrain_read`] hands readers the surface the user sees.
//!
//! ## Play
//!
//! Every edit made while Play runs comes undone when Play stops, whoever made
//! it, as a play test's edits do. The first edit of a Play session, a
//! script's or a tool's, snapshots the root's data, volume and water into
//! [`TerrainPlaySnapshot`], and [`restore_terrain_play_snapshot`] puts them
//! back on every path out of Play, so a game that digs, fills or clears its
//! terrain finds it as authored the next time. None of those edits gets an
//! undo entry: the restore would leave it pointing at terrain that no longer
//! exists.
//!
//! ## Undo
//!
//! Outside Play, [`TerrainCommandOrigin`] decides what outlives the edits:
//!
//! - An editor tool or MCP call records every command's bounds in one local
//!   `TerrainEditRecorder` (the shared one may hold an open brush stroke) and
//!   pushes the batch as one `Action::TerrainEdit` with the label it gave.
//!   While a brush stroke is open the batch still applies but gets no undo
//!   entry: a history step taken then lands inside the stroke's snapshots,
//!   which is also why undo waits for the stroke. The entry carries the
//!   water levels the batch changed too (a Sea Level fill, a fill that dries
//!   a column).
//! - A script edits for good, without an undo entry.
//!
//! A tool's Clear is refused, in Play or not, since it cannot be undone. A
//! raster, volume or water change outside Play that no undo entry carries
//! marks the Space unsaved, since terrain reaches disk only when the Space
//! is saved.

use bevy::prelude::*;
use eustress_common::terrain::api::{
    apply_terrain_command, command_bounds, command_needs_water, record_terrain_command, TerrainCommand,
    TerrainCommandEffect,
};
use eustress_common::terrain::{
    surface_data, TerrainBaked, TerrainConfig, TerrainData, TerrainDirtyChunks, TerrainEditRecorder, TerrainRoot,
    TerrainVolume, TerrainVoxelWater,
};

use crate::play_mode::PlayModeState;
use crate::undo::{Action, UndoStack};

/// Who asked for the edits.
#[derive(Clone, Debug)]
pub enum TerrainCommandOrigin {
    /// A Luau or Rune script in Play: no undo; the terrain is snapshotted before
    /// the first scripted edit of a Play session and restored when Play stops.
    Script,
    /// An editor tool or MCP call: one undo entry labelled `label` for the batch.
    /// During Play its edits come undone at Stop like a script's, without one.
    Tool { label: String },
}

/// The terrain as the first edit of the current Play session found it, which
/// [`restore_terrain_play_snapshot`] puts back when Play stops.
#[derive(Resource, Default)]
pub struct TerrainPlaySnapshot {
    /// The root, its data, and its volume and water (`None` where the root
    /// had none), or `None` before the session's first edit.
    pub saved: Option<(Entity, TerrainData, Option<TerrainVolume>, Option<TerrainVoxelWater>)>,
}

/// Why a tool's Clear fails.
const CLEAR_REFUSED: &str = "Clear cannot be undone, so an editor tool may not run it";

/// What a batch changed, for change detection and the unsaved marker.
#[derive(Clone, Copy, Debug, Default)]
struct BatchChanges {
    raster: bool,
    volume: bool,
    water: bool,
}

impl BatchChanges {
    fn note(&mut self, effect: &TerrainCommandEffect) {
        self.raster |= effect.height_rect.is_some() || effect.material_rect.is_some() || effect.cleared;
        self.volume |= effect.volume_edit.is_some() || effect.cleared;
        self.water |= effect.water_changed;
    }
}

/// Apply `commands` to the Space's terrain root, in order, marking dirty chunks. One result per command.
///
/// Every command fails alike when the Space has no terrain root, or more
/// than one. A root without a volume gets an empty one, and a root without
/// water gets a water component when a command writes water. See the module
/// docs for what outlives the edits, in Play and outside it.
pub fn apply_terrain_commands(
    world: &mut World,
    commands: Vec<TerrainCommand>,
    origin: TerrainCommandOrigin,
) -> Vec<Result<TerrainCommandEffect, String>> {
    if commands.is_empty() {
        return Vec::new();
    }
    let root = match terrain_root(world) {
        Ok(root) => root,
        Err(reason) => return refuse_all(commands.len(), &reason),
    };
    let tool = match &origin {
        TerrainCommandOrigin::Tool { .. } => true,
        TerrainCommandOrigin::Script => false,
    };
    let playing = play_is_running(world);
    // Only a tool's batch outside Play is undoable; Stop undoes every edit
    // made in Play.
    let label = match origin {
        TerrainCommandOrigin::Tool { label } if !playing => Some(label),
        TerrainCommandOrigin::Tool { label } => {
            debug!("Terrain: '{label}' ran during Play, which puts the terrain back at Stop, so it has no undo entry");
            None
        }
        TerrainCommandOrigin::Script => None,
    };
    // Before anything below adds a component, so Stop puts back exactly the
    // components the root had.
    if playing {
        snapshot_before_play_edit(world, root);
    }
    if world.get::<TerrainVolume>(root).is_none() {
        world.entity_mut(root).insert(TerrainVolume::default());
    }
    if world.get::<TerrainVoxelWater>(root).is_none() && commands.iter().any(command_needs_water) {
        world.entity_mut(root).insert(TerrainVoxelWater::default());
    }
    let stroke_open = world.get_resource::<TerrainEditRecorder>().is_some_and(TerrainEditRecorder::is_recording);

    let (config, results, recorded, changes) = {
        let mut query = world.query_filtered::<
            (&TerrainConfig, &mut TerrainData, &mut TerrainVolume, Option<&mut TerrainVoxelWater>),
            With<TerrainRoot>,
        >();
        let Ok((config, mut data, mut volume, mut water)) = query.get_mut(world, root) else {
            return refuse_all(commands.len(), "the terrain root has no config or data");
        };
        let config = config.clone();
        let mut recorder = TerrainEditRecorder::default();
        if let Some(label) = &label {
            recorder.begin(label.clone(), Some(root), &config, &data);
        }
        let mut results = Vec::with_capacity(commands.len());
        let mut changes = BatchChanges::default();
        for command in &commands {
            if tool && matches!(command, TerrainCommand::Clear) {
                results.push(Err(CLEAR_REFUSED.to_string()));
                continue;
            }
            // Change detection is set below from what the commands changed,
            // so a refused command leaves the root unflagged (autosave
            // rewrites the whole terrain for any flag).
            let (data, volume) = (data.bypass_change_detection(), volume.bypass_change_detection());
            if label.is_some() {
                record_terrain_command(&mut recorder, &config, data, volume, command);
            }
            if label.is_some() {
                // Water every command may change is recorded too, so a water
                // fill (or a fill that dries a column) undoes with the rest.
                if let Some((lo, hi)) = command_bounds(&config, command) {
                    recorder.record_water_rect(&config, data, water.as_deref(), lo.xz(), hi.xz());
                }
            }
            let water = water.as_mut().map(|water| water.bypass_change_detection());
            let result = apply_terrain_command(&config, data, volume, water, command);
            if let Ok(effect) = &result {
                changes.note(effect);
            }
            results.push(result);
        }
        if changes.raster {
            data.set_changed();
        }
        if changes.volume {
            volume.set_changed();
        }
        if changes.water {
            if let Some(water) = water.as_mut() {
                water.set_changed();
            }
        }
        let recorded = match label {
            Some(_) => recorder.finish_with_water(Some(root), &data, &volume, water.as_deref()),
            None => None,
        };
        (config, results, recorded, changes)
    };

    if let Some(mut dirty) = world.get_resource_mut::<TerrainDirtyChunks>() {
        for effect in results.iter().flatten() {
            mark_effect(&config, &mut dirty, effect);
        }
    }

    let mut undoable = false;
    if let (Some(label), Some(edit)) = (label, recorded) {
        if stroke_open {
            warn!("Terrain: '{label}' was applied while a brush stroke was open, so it has no undo entry");
        } else if let Some(mut undo) = world.get_resource_mut::<UndoStack>() {
            undo.push_labeled(
                edit.label.clone(),
                Action::TerrainEdit {
                    label: edit.label,
                    root: root.to_bits(),
                    tiles: edit.tiles,
                    bricks: edit.bricks,
                    water: edit.water,
                },
            );
            undoable = true;
        }
    }
    // Edits made in Play are put back at Stop.
    if (changes.raster || changes.volume || changes.water) && !undoable && !playing {
        mark_terrain_unsaved(world);
    }
    results
}

/// Live terrain read access for scripts and tools (surface data = the bake when layers exist).
///
/// `f` gets the root's config, its surface data (`surface_data`: the layer
/// bake when the root has layers, else its base), its volume (an empty one
/// when the root has none) and its water. `None` when the Space has no
/// terrain root, or more than one.
pub fn with_terrain_read<R>(
    world: &World,
    f: impl FnOnce(&TerrainConfig, &TerrainData, &TerrainVolume, Option<&TerrainVoxelWater>) -> R,
) -> Option<R> {
    // Only the components every root has go in the query: a query naming a
    // component type nothing has registered yet does not build.
    let mut roots = world.try_query_filtered::<Entity, With<TerrainRoot>>()?;
    let root = roots.single(world).ok()?;
    let config = world.get::<TerrainConfig>(root)?;
    let data = surface_data(world.get::<TerrainData>(root)?, world.get::<TerrainBaked>(root));
    let volume = world.get::<TerrainVolume>(root).unwrap_or(TerrainVolume::empty());
    Some(f(config, data, volume, world.get::<TerrainVoxelWater>(root)))
}

/// Put the terrain back as it was before the first edit of the Play session
/// that just stopped: the snapshot's data, volume and water go back on its
/// root (a volume or water the root did not have before is removed), every
/// chunk is marked dirty, and the snapshot is cleared. A root that is gone
/// (the terrain was replaced during the session) is left alone. Runs on every
/// transition back to Edit mode.
pub fn restore_terrain_play_snapshot(world: &mut World) {
    let saved = world
        .get_resource_mut::<TerrainPlaySnapshot>()
        .filter(|snapshot| snapshot.saved.is_some())
        .and_then(|mut snapshot| snapshot.saved.take());
    let Some((root, mut data, volume, water)) = saved else {
        return;
    };
    if world.get::<TerrainRoot>(root).is_none() {
        info!("Terrain: the Play session's terrain snapshot has no root to go back to; the terrain was replaced");
        return;
    }
    let config = world.get::<TerrainConfig>(root).cloned();
    // The slot palette follows the Space's slot table, not the session.
    if let Some(live) = world.get::<TerrainData>(root) {
        data.slot_palette = live.slot_palette.clone();
    }
    data.material_dirty = true;
    let mut entity = world.entity_mut(root);
    entity.insert(data);
    match volume {
        Some(volume) => {
            entity.insert(volume);
        }
        None => {
            entity.remove::<TerrainVolume>();
        }
    }
    match water {
        Some(water) => {
            entity.insert(water);
        }
        None => {
            entity.remove::<TerrainVoxelWater>();
        }
    }
    if let (Some(config), Some(mut dirty)) = (config, world.get_resource_mut::<TerrainDirtyChunks>()) {
        dirty.mark_all(&config);
    }
    info!("Terrain: put back as it was before the Play session's edits");
}

/// Drop a snapshot a Play session left behind as a new session starts from
/// Edit mode, so the new session snapshots the terrain as it is now.
pub fn clear_terrain_play_snapshot(snapshot: Option<ResMut<TerrainPlaySnapshot>>) {
    let Some(mut snapshot) = snapshot else {
        return;
    };
    if snapshot.saved.is_some() {
        warn!("Terrain: a Play snapshot outlived its session; the terrain keeps that session's edits");
        snapshot.saved = None;
    }
}

/// `count` copies of the refusal `reason`, one per command.
fn refuse_all(count: usize, reason: &str) -> Vec<Result<TerrainCommandEffect, String>> {
    (0..count).map(|_| Err(reason.to_string())).collect()
}

/// The Space's one terrain root, or why there is none to edit.
fn terrain_root(world: &mut World) -> Result<Entity, String> {
    let mut roots = world.query_filtered::<Entity, With<TerrainRoot>>();
    let mut found = roots.iter(world);
    match (found.next(), found.next()) {
        (Some(root), None) => Ok(root),
        (None, _) => Err("there is no terrain to edit".to_string()),
        (Some(_), Some(_)) => Err("more than one terrain is loaded".to_string()),
    }
}

/// A Play session is running, playing or paused.
fn play_is_running(world: &World) -> bool {
    world
        .get_resource::<State<PlayModeState>>()
        .is_some_and(|state| *state.get() != PlayModeState::Editing)
}

/// How [`apply_terrain_commands`] keeps a `Tool` batch applied now, for a
/// caller that reports it (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolEditRecord {
    /// One undo entry.
    Undo,
    /// No undo entry: Play is running, and the batch comes undone when it
    /// stops.
    RolledBackAtStop,
    /// No undo entry: a terrain brush stroke was open.
    NotUndoable,
}

/// What a `Tool` batch applied now becomes (see [`ToolEditRecord`]).
pub fn tool_edit_record(world: &World) -> ToolEditRecord {
    if play_is_running(world) {
        ToolEditRecord::RolledBackAtStop
    } else if world.get_resource::<TerrainEditRecorder>().is_some_and(TerrainEditRecorder::is_recording) {
        ToolEditRecord::NotUndoable
    } else {
        ToolEditRecord::Undo
    }
}

/// Snapshot `root`'s terrain for the running Play session, unless an earlier
/// edit of the session already did. A snapshot whose root is gone (the
/// terrain was replaced mid-session) gives way to one of the live root.
fn snapshot_before_play_edit(world: &mut World, root: Entity) {
    let taken = world
        .get_resource::<TerrainPlaySnapshot>()
        .and_then(|snapshot| snapshot.saved.as_ref())
        .is_some_and(|(saved, ..)| *saved == root || world.get::<TerrainRoot>(*saved).is_some());
    if taken {
        return;
    }
    let Some(data) = world.get::<TerrainData>(root).cloned() else {
        return;
    };
    let volume = world.get::<TerrainVolume>(root).cloned();
    let water = world.get::<TerrainVoxelWater>(root).cloned();
    let mut snapshot = world.get_resource_or_insert_with(TerrainPlaySnapshot::default);
    snapshot.saved = Some((root, data, volume, water));
    info!("Terrain: snapshotted before the Play session's first edit; Stop puts it back");
}

/// Mark in `dirty` the chunks `effect` changed.
fn mark_effect(config: &TerrainConfig, dirty: &mut TerrainDirtyChunks, effect: &TerrainCommandEffect) {
    if effect.cleared {
        dirty.mark_all(config);
        return;
    }
    if let Some((min, max)) = effect.height_rect {
        dirty.mark_world_rect(config, min, max);
    }
    if let Some((min, max)) = effect.material_rect {
        dirty.mark_world_rect_materials(config, min, max);
    }
    if let Some(edit) = &effect.volume_edit {
        dirty.mark_volume_edit(config, edit);
    }
}

/// Mark the Space unsaved after a terrain change no undo entry carries, the
/// way terrain undo does: the unsaved marker compares the undo stack's push
/// sequence with the one saved last, which such a change does not move.
fn mark_terrain_unsaved(world: &mut World) {
    let sequence = world.get_resource::<UndoStack>().map(UndoStack::sequence).unwrap_or(0);
    if let Some(mut state) = world.get_resource_mut::<crate::ui::StudioState>() {
        if state.saved_undo_sequence == sequence {
            state.saved_undo_sequence = sequence.wrapping_sub(1);
        }
        state.has_unsaved_changes = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::terrain::api::{TerrainFill, TerrainSculptMode};
    use eustress_common::terrain::TerrainMaterial;

    /// 3 x 3 chunks of 32 m at 16 cells: a 2 m lattice under a 48 x 48
    /// raster spanning world -32..64, heights 0..64 m, flat at 0.
    fn world_with_terrain() -> (World, Entity) {
        let config = TerrainConfig {
            chunk_size: 32.0,
            chunk_resolution: 16,
            chunks_x: 1,
            chunks_z: 1,
            height_scale: 64.0,
            height_offset: 0.0,
            ..TerrainConfig::default()
        };
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        let mut world = World::new();
        world.init_resource::<TerrainDirtyChunks>();
        world.init_resource::<UndoStack>();
        let root = world.spawn((TerrainRoot, config, data, TerrainVolume::default())).id();
        (world, root)
    }

    fn ball(center: Vec3, fill: TerrainFill) -> TerrainCommand {
        TerrainCommand::FillBall { center, radius: 4.0, fill }
    }

    #[test]
    fn a_tool_batch_is_one_undo_entry_and_marks_its_chunks() {
        let (mut world, root) = world_with_terrain();
        let results = apply_terrain_commands(
            &mut world,
            vec![
                ball(Vec3::new(0.0, 8.0, 0.0), TerrainFill::Material(TerrainMaterial::Rock)),
                TerrainCommand::Sculpt {
                    mode: TerrainSculptMode::Raise,
                    center: Vec3::new(10.0, 0.0, 10.0),
                    radius: 6.0,
                    strength: 1.0,
                },
                TerrainCommand::Clear,
            ],
            TerrainCommandOrigin::Tool { label: "Fill Terrain".to_string() },
        );
        assert_eq!(results.len(), 3);
        assert!(results[0].is_ok() && results[1].is_ok(), "{results:?}");
        assert!(results[2].is_err(), "a tool may not clear");

        let undo = world.resource::<UndoStack>();
        assert_eq!(undo.history().len(), 1, "one entry for the whole batch");
        assert_eq!(undo.sequence(), 1);
        let Some(Action::TerrainEdit { label, root: edited, tiles, bricks, .. }) = undo.history().front() else {
            panic!("the entry is a terrain edit");
        };
        assert_eq!(label, "Fill Terrain");
        assert_eq!(*edited, root.to_bits());
        assert!(!tiles.is_empty(), "it holds the sculpt's tiles");
        assert!(!bricks.is_empty(), "and the fill's bricks");

        let dirty = world.resource::<TerrainDirtyChunks>();
        assert!(!dirty.remesh.is_empty() && !dirty.recollide.is_empty(), "the edited chunks rebuild");
        assert!(!world.get::<TerrainData>(root).unwrap().sparse_surface, "the refused Clear left the ground");
    }

    #[test]
    fn scripts_in_play_are_snapshotted_once_and_put_back_at_stop() {
        let (mut world, root) = world_with_terrain();
        world.insert_resource(State::new(PlayModeState::Playing));
        let original = world.get::<TerrainData>(root).unwrap().height_cache.clone();

        for step in 0..2 {
            let x = step as f32 * 12.0;
            let results = apply_terrain_commands(
                &mut world,
                vec![
                    TerrainCommand::Sculpt {
                        mode: TerrainSculptMode::Raise,
                        center: Vec3::new(x, 0.0, 0.0),
                        radius: 5.0,
                        strength: 1.0,
                    },
                    ball(Vec3::new(x, 10.0, 0.0), TerrainFill::Material(TerrainMaterial::Sand)),
                    TerrainCommand::FillBlock {
                        center: Vec3::new(x, 2.0, 0.0),
                        rotation: Quat::IDENTITY,
                        size: Vec3::splat(4.0),
                        fill: TerrainFill::Water,
                    },
                ],
                TerrainCommandOrigin::Script,
            );
            assert!(results.iter().all(Result::is_ok), "{results:?}");
        }
        assert!(world.resource::<UndoStack>().history().is_empty(), "scripts leave no undo entry");
        let snapshot = world.get_resource::<TerrainPlaySnapshot>().expect("the first edit made a snapshot");
        let (saved_root, saved_data, saved_volume, saved_water) = snapshot.saved.as_ref().expect("snapshotted");
        assert_eq!(*saved_root, root);
        assert_eq!(saved_data.height_cache, original, "taken before the first edit, not the second");
        assert!(saved_volume.as_ref().is_some_and(TerrainVolume::is_empty));
        assert!(saved_water.is_none(), "the root had no water before Play");
        assert_ne!(world.get::<TerrainData>(root).unwrap().height_cache, original);
        assert!(!world.get::<TerrainVolume>(root).unwrap().is_empty());
        assert!(world.get::<TerrainVoxelWater>(root).is_some(), "the water fill gave the root water");

        {
            let mut dirty = world.resource_mut::<TerrainDirtyChunks>();
            dirty.remesh.clear();
            dirty.recollide.clear();
        }
        restore_terrain_play_snapshot(&mut world);
        assert_eq!(world.get::<TerrainData>(root).unwrap().height_cache, original);
        assert!(world.get::<TerrainVolume>(root).is_some_and(TerrainVolume::is_empty));
        assert!(world.get::<TerrainVoxelWater>(root).is_none(), "water the session added goes");
        assert!(world.resource::<TerrainPlaySnapshot>().saved.is_none(), "the snapshot is used up");
        assert!(!world.resource::<TerrainDirtyChunks>().is_empty(), "every chunk rebuilds");
    }

    #[test]
    fn a_tool_edit_during_play_is_put_back_at_stop_without_an_undo_entry() {
        let (mut world, root) = world_with_terrain();
        // Paused is still a Play session.
        world.insert_resource(State::new(PlayModeState::Paused));
        let results = apply_terrain_commands(
            &mut world,
            vec![ball(Vec3::new(0.0, 8.0, 0.0), TerrainFill::Material(TerrainMaterial::Rock)), TerrainCommand::Clear],
            TerrainCommandOrigin::Tool { label: "Fill Terrain".to_string() },
        );
        assert!(results[0].is_ok(), "{results:?}");
        assert!(results[1].is_err(), "a tool may not clear, in Play or not");
        assert!(world.resource::<UndoStack>().history().is_empty(), "Stop undoes it instead");
        assert!(!world.get::<TerrainVolume>(root).unwrap().is_empty());
        assert!(world.resource::<TerrainPlaySnapshot>().saved.is_some(), "the tool's edit was snapshotted");

        restore_terrain_play_snapshot(&mut world);
        assert!(world.get::<TerrainVolume>(root).is_some_and(TerrainVolume::is_empty));
        assert!(world.resource::<TerrainPlaySnapshot>().saved.is_none());
    }

    #[test]
    fn without_a_terrain_every_command_fails_and_reads_find_nothing() {
        let mut world = World::new();
        let results = apply_terrain_commands(
            &mut world,
            vec![TerrainCommand::Clear, ball(Vec3::ZERO, TerrainFill::Air)],
            TerrainCommandOrigin::Script,
        );
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(Result::is_err));
        assert!(with_terrain_read(&world, |_, _, _, _| ()).is_none());

        let (world, _) = world_with_terrain();
        let read = with_terrain_read(&world, |config, data, volume, water| {
            (config.chunk_resolution, data.cache_width, volume.is_empty(), water.is_none())
        });
        assert_eq!(read, Some((16, 48, true, true)));
    }
}
