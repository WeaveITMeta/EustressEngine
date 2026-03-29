//! # EustressStream Change Queue
//!
//! Replaces the former Apache Iggy integration with the embedded
//! `eustress-stream` crate. The public API is unchanged so all call sites
//! (engine, CLI, scenarios) continue to compile without modification.
//!
//! ## Design
//!
//! The hot path is **sub-microsecond**:
//!   1. ECS system calls `queue.send_delta(delta)` — serialises the delta and
//!      calls `Producer::send_bytes()` directly (no channel, no network hop).
//!   2. EustressStream dispatches synchronously to all in-process subscribers
//!      (TOML materializer, property panel, Explorer tree, …).
//!   3. No external server process is required; everything lives in the same
//!      address space as the Bevy app.
//!
//! ## Feature Gate
//! Compiled only when the `iggy-streaming` feature is enabled.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bytes::Bytes;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use tracing::{info, warn};

use eustress_stream::{EustressStream, OwnedMessage, Producer, StreamConfig};

use crate::iggy_delta::{
    AgentCommand, AgentObservation, SceneDelta,
    IGGY_DEFAULT_URL, IGGY_STREAM_NAME,
    IGGY_TOPIC_AGENT_COMMANDS, IGGY_TOPIC_AGENT_OBSERVATIONS, IGGY_TOPIC_SCENE_DELTAS,
};

// ─────────────────────────────────────────────────────────────────────────────
// IggyConfig  (kept for API compatibility — url/stream_name fields are ignored)
// ─────────────────────────────────────────────────────────────────────────────

/// Connection and streaming configuration.
///
/// `url` and `stream_name` are retained for backward compatibility but are
/// ignored by EustressStream (no external server is required).
#[derive(Debug, Clone, Resource)]
pub struct IggyConfig {
    /// Legacy Iggy URL — retained for API compatibility, not used.
    pub url: String,
    /// Legacy stream name — retained for API compatibility, not used.
    pub stream_name: String,
    pub topic_scene_deltas: String,
    pub topic_agent_commands: String,
    pub topic_agent_observations: String,
    pub batch_size: usize,
    pub delta_linger_ms: u64,
    pub sim_linger_ms: u64,
    pub agent_poll_ms: u64,
    /// Ring-buffer capacity (number of messages retained in memory).
    pub channel_capacity: usize,
    /// When true, delta serialize errors are silently swallowed.
    pub drop_on_full: bool,
    pub scene_delta_partitions: u32,
    pub sim_result_partitions: u32,
}

impl Default for IggyConfig {
    fn default() -> Self {
        Self {
            url: IGGY_DEFAULT_URL.to_string(),
            stream_name: IGGY_STREAM_NAME.to_string(),
            topic_scene_deltas: IGGY_TOPIC_SCENE_DELTAS.to_string(),
            topic_agent_commands: IGGY_TOPIC_AGENT_COMMANDS.to_string(),
            topic_agent_observations: IGGY_TOPIC_AGENT_OBSERVATIONS.to_string(),
            batch_size: 512,
            delta_linger_ms: 1,
            sim_linger_ms: 0,
            agent_poll_ms: 10,
            channel_capacity: 65_536,
            drop_on_full: true,
            scene_delta_partitions: 8,
            sim_result_partitions: 4,
        }
    }
}

impl IggyConfig {
    pub fn scene_partitions(&self) -> u64 {
        self.scene_delta_partitions.max(1) as u64
    }
    pub fn sim_partitions(&self) -> u32 {
        self.sim_result_partitions.max(1)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// IggyChangeQueue
// ─────────────────────────────────────────────────────────────────────────────

/// Bevy Resource — fire-and-forget sink for ECS mutation deltas.
///
/// ECS systems call `queue.send_delta(delta)` with <1 µs cost.
/// Dispatch is **synchronous and in-process** — no background thread, no TCP.
#[derive(Resource)]
pub struct IggyChangeQueue {
    /// Shared EustressStream — clone it to subscribe to any topic.
    pub stream: EustressStream,
    delta_producer: Producer,
    observation_producer: Producer,
    /// Receives decoded `AgentCommand`s from the `agent_commands` topic.
    pub command_rx: Arc<tokio::sync::Mutex<UnboundedReceiver<AgentCommand>>>,
    seq: Arc<AtomicU64>,
    session_start: Instant,
    drop_on_full: bool,
}

impl IggyChangeQueue {
    /// Send a scene delta. Non-blocking (<1 µs).
    pub fn send_delta(&self, delta: SceneDelta) {
        match delta.to_bytes() {
            Ok(b) => {
                self.delta_producer.send_bytes(Bytes::from(b));
            }
            Err(e) => {
                if !self.drop_on_full {
                    warn!("IggyChangeQueue: delta serialize failed: {e}");
                }
            }
        }
    }

    /// Send an agent observation to in-process subscribers.
    pub fn send_observation(&self, obs: AgentObservation) {
        match rkyv::to_bytes::<rkyv::rancor::Error>(&obs) {
            Ok(b) => {
                self.observation_producer.send_bytes(Bytes::from(b.to_vec()));
            }
            Err(e) => warn!("IggyChangeQueue: observation serialize failed: {e}"),
        }
    }

    /// Next monotonically increasing sequence number.
    pub fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }

    /// Milliseconds elapsed since the session started.
    pub fn now_ms(&self) -> u64 {
        self.session_start.elapsed().as_millis() as u64
    }

    /// Wall-clock Unix timestamp in ms.
    pub fn unix_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// init_iggy — bootstrap (no external server required)
// ─────────────────────────────────────────────────────────────────────────────

/// Initialise the EustressStream and return a ready `IggyChangeQueue`.
///
/// This is now infallible in practice (no network call), but returns `Result`
/// for API compatibility with existing call sites.
pub async fn init_iggy(config: &IggyConfig) -> Result<IggyChangeQueue, String> {
    let ring_cap = config.channel_capacity.next_power_of_two();
    let stream = EustressStream::new(
        StreamConfig::default()
            .with_ring_capacity(ring_cap)
            .in_memory(),
    );

    let delta_producer       = stream.producer(IGGY_TOPIC_SCENE_DELTAS);
    let observation_producer = stream.producer(IGGY_TOPIC_AGENT_OBSERVATIONS);

    // Subscribe to incoming agent commands and forward to a tokio mpsc channel
    // so the existing `poll_agent_commands` Bevy system works unchanged.
    let (cmd_tx, cmd_rx) = unbounded_channel::<AgentCommand>();
    stream
        .subscribe_owned(IGGY_TOPIC_AGENT_COMMANDS, move |msg: OwnedMessage| {
            match rkyv::from_bytes::<AgentCommand, rkyv::rancor::Error>(msg.data.as_ref()) {
                Ok(cmd) => {
                    let _ = cmd_tx.send(cmd);
                }
                Err(e) => warn!("EustressStream: command deserialize: {e}"),
            }
        })
        .map_err(|e| format!("subscribe agent_commands: {e}"))?;

    info!("EustressStream: streaming ready (in-process, no external server).");

    Ok(IggyChangeQueue {
        stream,
        delta_producer,
        observation_producer,
        command_rx: Arc::new(tokio::sync::Mutex::new(cmd_rx)),
        seq: Arc::new(AtomicU64::new(0)),
        session_start: Instant::now(),
        drop_on_full: config.drop_on_full,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// IggyPlugin
// ─────────────────────────────────────────────────────────────────────────────

/// Bevy plugin: initialises EustressStream and registers ECS systems.
pub struct IggyPlugin;

impl Plugin for IggyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_iggy_queue)
           .add_systems(Update, (poll_agent_commands, emit_lifecycle_deltas));
    }
}

fn setup_iggy_queue(mut commands: Commands, config: Option<Res<IggyConfig>>) {
    let config = config.map(|c| c.clone()).unwrap_or_default();

    // Run init_iggy on a background thread so we don't need an existing tokio
    // runtime on the main thread during Bevy startup.
    let result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio rt");
        rt.block_on(init_iggy(&config))
    })
    .join()
    .unwrap_or_else(|_| Err("EustressStream init thread panicked".to_string()));

    match result {
        Ok(queue) => {
            info!("IggyPlugin: IggyChangeQueue ready.");
            commands.insert_resource(queue);
        }
        Err(e) => {
            warn!("IggyPlugin: stream init failed ({e}).");
        }
    }
}

fn poll_agent_commands(
    queue: Option<Res<IggyChangeQueue>>,
    mut commands: Commands,
) {
    let Some(queue) = queue else { return };
    let Ok(mut rx) = queue.command_rx.try_lock() else { return };
    while let Ok(cmd) = rx.try_recv() {
        commands.trigger(IncomingAgentCommand(cmd));
    }
}

/// Bevy observer event — fired once per incoming `AgentCommand`.
#[derive(Event)]
pub struct IncomingAgentCommand(pub AgentCommand);

// ─────────────────────────────────────────────────────────────────────────────
// Lifecycle delta emission
// ─────────────────────────────────────────────────────────────────────────────

fn emit_lifecycle_deltas(
    queue: Option<Res<IggyChangeQueue>>,
    added: Query<Entity, Added<bevy::prelude::Name>>,
    mut removed: RemovedComponents<bevy::prelude::Name>,
) {
    let Some(queue) = queue else { return };

    for entity in added.iter() {
        let seq = queue.next_seq();
        let ts  = IggyChangeQueue::unix_ms();
        queue.send_delta(SceneDelta::lifecycle(
            entity.to_bits(),
            crate::iggy_delta::DeltaKind::PartAdded,
            seq,
            ts,
        ));
    }

    for entity in removed.read() {
        let seq = queue.next_seq();
        let ts  = IggyChangeQueue::unix_ms();
        queue.send_delta(SceneDelta::lifecycle(
            entity.to_bits(),
            crate::iggy_delta::DeltaKind::PartRemoved,
            seq,
            ts,
        ));
    }
}
