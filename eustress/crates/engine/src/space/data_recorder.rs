//! P2 — the Data Platform **Recorder** engine seam.
//!
//! The single durable writer for sensor samples: producers (sim taps, hardware
//! adapters, a `sensor.<name>` stream bridge — all future) push samples onto a
//! bounded, **drop-on-full** channel via [`SensorRecorderSender`]; one Bevy
//! system drains them into per-series [`RecorderBuffer`]s and batch-flushes to
//! the WorldDb `timeseries` partition on a cadence, suppressed during load.
//!
//! The batching / flush / drop-counting logic itself lives in (and is unit-
//! tested in) the engine-free `eustress-data-store` crate; this module is only
//! the Bevy wiring — the channel, the resource, the flush system, and the
//! `WorldDbHandle` + `LoadInProgress` gates. Gated on `data` (+ `world-db`,
//! since it is registered from `WorldDbPlugin`).

use std::collections::HashMap;

use bevy::prelude::*;
use crossbeam_channel::{bounded, Receiver, Sender};
use eustress_data_store::RecorderBuffer;

use super::file_loader::LoadInProgress;
use super::world_db_plugin::WorldDbHandle;
use crate::play_mode::PlayModeState;
use crate::space::DataRecording;

/// One sensor sample bound for the `timeseries` partition.
#[derive(Clone, Debug)]
pub struct SensorSample {
    /// Series name (the `timeseries` key prefix, e.g. `"sensor.psi"`).
    pub series: String,
    /// Timestamp (caller's monotonic unit, typically sim-time micros/nanos).
    pub ts: u64,
    /// Disambiguator for samples sharing a `ts` (use 0 if collisions are impossible).
    pub seq: u32,
    /// The f64 sample value.
    pub value: f64,
}

/// Producer handle (a cloneable resource). Sim taps, hardware adapters, and a
/// future `sensor.<name>` stream bridge call [`SensorRecorderSender::record`].
/// Drop-on-full: a record never blocks a high-rate probe.
#[derive(Resource, Clone)]
pub struct SensorRecorderSender(Sender<SensorSample>);

impl SensorRecorderSender {
    /// Queue one sample. Returns `false` if the channel is full (the sample is
    /// dropped — bounded loss, never a stall).
    pub fn record(&self, series: impl Into<String>, ts: u64, seq: u32, value: f64) -> bool {
        self.0
            .try_send(SensorSample {
                series: series.into(),
                ts,
                seq,
                value,
            })
            .is_ok()
    }
}

/// The recorder's working state: the channel receiver, per-series buffers, and
/// the flush cadence.
#[derive(Resource)]
struct RecorderState {
    rx: Receiver<SensorSample>,
    buffers: HashMap<String, RecorderBuffer>,
    /// Max buffered samples per series before drop-on-full.
    cap: usize,
    /// Flush every N frames (so a kHz probe isn't a per-frame write storm).
    flush_every: u32,
    frame: u32,
}

/// Data Platform Recorder plugin (P2). Registered from [`WorldDbPlugin`] under
/// `cfg(feature = "data")`.
pub struct DataRecorderPlugin;

impl Plugin for DataRecorderPlugin {
    fn build(&self, app: &mut App) {
        // Bounded channel — producers drop-on-full, the recorder never stalls.
        let (tx, rx) = bounded::<SensorSample>(16_384);
        app.insert_resource(SensorRecorderSender(tx))
            .insert_resource(RecorderState {
                rx,
                buffers: HashMap::new(),
                cap: 65_536,
                flush_every: 30,
                frame: 0,
            })
            .insert_resource(DataRecording::default())
            .add_systems(
                Update,
                (forget_recording_on_space_change, drain_and_flush_recorder, record_sampler).chain(),
            )
            .add_systems(OnEnter(PlayModeState::Editing), flush_recorder_on_stop);
    }
}

/// Move queued samples into their per-series buffers.
fn drain_queue(state: &mut RecorderState) {
    let drained: Vec<SensorSample> = state.rx.try_iter().collect();
    let cap = state.cap;
    for s in drained {
        state
            .buffers
            .entry(s.series.clone())
            .or_insert_with(|| RecorderBuffer::new(s.series.clone(), cap))
            .push(s.ts, s.seq, s.value);
    }
}

/// Stop: write everything the run recorded now, into the Space it was
/// recorded in, rather than at the next cadence tick, by when the Space may
/// have changed.
fn flush_recorder_on_stop(mut state: ResMut<RecorderState>, db: Res<WorldDbHandle>) {
    drain_queue(&mut state);
    let Some(db) = db.0.clone() else {
        return;
    };
    for buf in state.buffers.values_mut() {
        if !buf.is_empty() {
            if let Err(e) = buf.flush(db.as_ref()) {
                warn!(target: "eustress_engine::data", "recorder flush at Stop failed: {e}");
            }
        }
    }
}

/// A Space switch: the outgoing Space's database is already closed, so samples
/// still waiting cannot reach it, and they must never land in the incoming
/// Space's `timeseries`. Drop them (Stop writes everything first, so this only
/// happens when a switch comes before the next flush), and disarm Record: its
/// target was an entity of the outgoing Space.
fn forget_recording_on_space_change(
    space_root: Option<Res<crate::space::SpaceRoot>>,
    mut rec: ResMut<DataRecording>,
    mut state: ResMut<RecorderState>,
) {
    let Some(space_root) = space_root else {
        return;
    };
    if !space_root.is_changed() {
        return;
    }
    let queued = state.rx.try_iter().count();
    let buffered: usize = state.buffers.values().map(|b| b.len()).sum();
    state.buffers.clear();
    state.frame = 0;
    if queued + buffered > 0 {
        warn!(
            target: "eustress_engine::data",
            "recorder: {} samples from the previous Space were not saved; it closed before the next flush",
            queued + buffered
        );
    }
    if rec.active || rec.target.is_some() {
        info!(target: "eustress_engine::data", "recorder: Record disarmed by the Space switch");
    }
    *rec = DataRecording::default();
}

/// Producer A — sample the armed target Part's `Transform` each Play frame and
/// push x/y/z to the Recorder. Gated to `Playing` so editing never records;
/// no-op until the `data:record` action arms a target.
fn record_sampler(
    mut rec: ResMut<DataRecording>,
    play: Option<Res<State<PlayModeState>>>,
    time: Res<Time>,
    sender: Res<SensorRecorderSender>,
    transforms: Query<&Transform>,
    alive: Query<Entity>,
    instances: Query<&eustress_common::classes::Instance>,
) {
    if !rec.active {
        return;
    }
    let playing = play.map(|p| matches!(p.get(), PlayModeState::Playing)).unwrap_or(false);
    if !playing {
        return;
    }
    let Some(target) = rec.target else {
        return;
    };
    let Ok(tf) = transforms.get(target) else {
        // The target is gone (deleted, or Stop restored a different entity):
        // disarm rather than keep "recording" nothing.
        if !alive.contains(target) {
            warn!(target: "eustress_engine::data", "recorder: the recorded part no longer exists; Record disarmed");
            rec.active = false;
            rec.target = None;
        }
        return;
    };
    let ts = time.elapsed().as_millis() as u64;
    let seq = rec.seq;
    // Series are keyed by the instance's stable UUID, so runs recorded before
    // and after a reload land in the same series; an entity index changes.
    let base = match instances.get(target).map(|i| i.uuid.as_str()) {
        Ok(uuid) if !uuid.is_empty() => format!("record.{uuid}"),
        _ => format!("record.e{}", target.index()),
    };
    sender.record(format!("{base}.x"), ts, seq, tf.translation.x as f64);
    sender.record(format!("{base}.y"), ts, seq, tf.translation.y as f64);
    sender.record(format!("{base}.z"), ts, seq, tf.translation.z as f64);
    rec.seq = rec.seq.wrapping_add(1);
}

/// Drain queued samples into per-series buffers, then batch-flush to the
/// `timeseries` partition on a cadence (skipped while a load is in progress, to
/// match the transform-mirror's `LoadInProgress` gate).
fn drain_and_flush_recorder(
    mut state: ResMut<RecorderState>,
    db: Res<WorldDbHandle>,
    load: Option<Res<LoadInProgress>>,
) {
    drain_queue(&mut state);

    // Never persist during cold-load + the quiescence window.
    if load.map(|l| l.active).unwrap_or(false) {
        return;
    }
    state.frame = state.frame.wrapping_add(1);
    if state.frame % state.flush_every.max(1) != 0 {
        return;
    }
    let Some(db) = db.0.clone() else {
        return;
    };
    for buf in state.buffers.values_mut() {
        if !buf.is_empty() {
            if let Err(e) = buf.flush(db.as_ref()) {
                warn!(target: "eustress_engine::data", "recorder flush failed: {e}");
            }
        }
    }
}
