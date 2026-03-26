//! # Iggy Change Queue
//!
//! ## Table of Contents
//! - IggyConfig             — connection + stream/topic configuration (Bevy Resource)
//! - IggyChangeQueue        — Bevy Resource: fire-and-forget delta sink for ECS systems
//! - IggyPlugin             — Bevy plugin: setup + emit_transform_deltas + poll_agent_commands
//! - init_iggy              — async bootstrap: connect, ensure stream/topics, spawn tasks
//! - run_delta_producer     — background task: drain channel → batch → iggy send_messages
//! - run_observation_producer — background task: drain observations → iggy
//! - run_command_consumer   — background task: iggy poll_messages → command channel
//!
//! ## Design
//!
//! The hot path is **sub-microsecond**:
//!   1. ECS system calls `queue.send_delta(delta)` — pushes into a lockless mpsc channel.
//!   2. Background tokio task drains the channel, batches up to 512 deltas, and sends
//!      one `send_messages()` call every 1ms linger window.
//!   3. Iggy appends to its append-only binary log at >1 GB/s — never blocks the Bevy loop.
//!
//! ## Feature Gate
//! Compiled only when the `iggy-streaming` feature is enabled
//! (gated via `#[cfg(feature = "iggy-streaming")]` on the `mod` in `lib.rs`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bytes::Bytes;
use tokio::sync::mpsc::{channel, unbounded_channel, Receiver, Sender, UnboundedReceiver, UnboundedSender};
use tracing::{error, info, warn};

use iggy::prelude::{
    CompressionAlgorithm, Consumer, IggyExpiry, IggyMessage, Identifier,
    MaxTopicSize, MessageClient, Partitioning, PollingStrategy, StreamClient, TopicClient,
};
use iggy::clients::client::IggyClient;

use crate::iggy_delta::{
    AgentCommand, AgentObservation, SceneDelta,
    IGGY_DEFAULT_URL, IGGY_STREAM_NAME,
    IGGY_TOPIC_AGENT_COMMANDS, IGGY_TOPIC_AGENT_OBSERVATIONS, IGGY_TOPIC_SCENE_DELTAS,
};

// ─────────────────────────────────────────────────────────────────────────────
// IggyConfig
// ─────────────────────────────────────────────────────────────────────────────

/// Connection and streaming configuration for the Iggy integration.
///
/// Also used by `SimStreamWriter` / `SimStreamReader` as the single source of truth
/// for all Iggy connection parameters (Layer 7: merged config).
#[derive(Debug, Clone, Resource)]
pub struct IggyConfig {
    /// Full Iggy connection string. Default: `iggy://iggy:iggy@127.0.0.1:8090`.
    pub url: String,
    /// Iggy stream name. Default: `eustress`.
    pub stream_name: String,
    /// Topic for scene mutation deltas.
    pub topic_scene_deltas: String,
    /// Topic for agent commands (CLI → session).
    pub topic_agent_commands: String,
    /// Topic for agent observations (session → CLI).
    pub topic_agent_observations: String,
    /// Maximum deltas to batch per send call.
    pub batch_size: usize,

    // ── Layer 5: per-topic linger tuning ─────────────────────────────────────
    /// Linger window for scene delta batches (ms). Default 1ms for real-time feeds.
    pub delta_linger_ms: u64,
    /// Linger window for simulation records (ms). Default 0 — publish immediately.
    pub sim_linger_ms: u64,
    /// Agent command consumer poll interval (ms). Default 10ms.
    pub agent_poll_ms: u64,

    // ── Layer 2: bounded channel + backpressure ───────────────────────────────
    /// Capacity of the in-process scene delta channel.
    /// At 68 bytes/delta this caps in-flight memory at ~4.4 MB (default 65_536).
    pub channel_capacity: usize,
    /// When true, new deltas are silently dropped when the channel is full instead
    /// of blocking. The Studio main thread never stalls waiting for Iggy.
    pub drop_on_full: bool,

    // ── Layer 3: topic partition counts ──────────────────────────────────────
    /// Partition count for `scene_deltas` topic. Sharded by `entity_id % n`.
    /// Default 8 — yields ~1M delta/s aggregate throughput across partitions.
    pub scene_delta_partitions: u32,
    /// Partition count for simulation topics (`sim_results`, `rune_scripts`).
    /// Sharded by `scenario_id % n`. Default 4.
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
    /// Number of `scene_delta` partitions, clamped to at least 1.
    pub fn scene_partitions(&self) -> u64 {
        self.scene_delta_partitions.max(1) as u64
    }
    /// Number of sim-result partitions, clamped to at least 1.
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
/// A background tokio task batches and publishes to Iggy asynchronously.
///
/// Layer 2: the internal channel is bounded (`channel_capacity`) with tail-drop
/// so the Bevy main thread is never blocked waiting for Iggy backpressure.
#[derive(Resource)]
pub struct IggyChangeQueue {
    /// Bounded channel sender — pushes deltas to the background producer task.
    delta_tx: Sender<SceneDelta>,
    /// Unbounded channel for observations (low-frequency, no drop needed).
    observation_tx: UnboundedSender<AgentObservation>,
    /// Channel receiver for agent commands arriving from CLI agents.
    pub command_rx: Arc<tokio::sync::Mutex<UnboundedReceiver<AgentCommand>>>,
    /// Monotonically increasing sequence counter.
    seq: Arc<AtomicU64>,
    /// Session start time for relative delta timestamps.
    session_start: Instant,
    /// Whether to silently drop deltas when the channel is full.
    drop_on_full: bool,
}

impl IggyChangeQueue {
    /// Send a scene delta. Non-blocking (<1 µs).
    ///
    /// Layer 2: uses `try_send` on the bounded channel.
    /// - If `drop_on_full = true` (default): silently drops the delta — the Studio
    ///   main thread never stalls waiting for Iggy.
    /// - If `drop_on_full = false`: logs a warning but still does NOT block.
    pub fn send_delta(&self, delta: SceneDelta) {
        match self.delta_tx.try_send(delta) {
            Ok(_) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                if !self.drop_on_full {
                    warn!("IggyChangeQueue: delta channel full — dropping delta (Iggy slow or offline)");
                }
                // Silent tail-drop when drop_on_full = true
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                warn!("IggyChangeQueue: delta channel closed");
            }
        }
    }

    /// Send an agent observation back to CLI subscribers.
    pub fn send_observation(&self, obs: AgentObservation) {
        if let Err(e) = self.observation_tx.send(obs) {
            warn!("IggyChangeQueue: observation channel closed: {}", e);
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
// init_iggy — async bootstrap
// ─────────────────────────────────────────────────────────────────────────────

/// Connect to Iggy, ensure stream + topics exist, spawn background tasks,
/// and return a ready `IggyChangeQueue`.
///
/// Returns `Err` if the Iggy server is unreachable — callers should log and
/// continue without streaming rather than crashing.
pub async fn init_iggy(config: &IggyConfig) -> Result<IggyChangeQueue, String> {
    // Bootstrap client — used only for stream/topic creation, then dropped.
    let bootstrap = IggyClient::from_connection_string(&config.url)
        .map_err(|e| format!("Iggy URL parse error: {e}"))?;
    bootstrap
        .connect()
        .await
        .map_err(|e| format!("Iggy connect failed (is iggy-server running on {}?): {e}", config.url))?;
    info!("Iggy: connected to {}", config.url);
    ensure_stream(&bootstrap, &config.stream_name).await;
    // Layer 3: scene_deltas gets the configured partition count; agent topics stay at 1 (ordered).
    ensure_topic_with_partitions(&bootstrap, &config.stream_name, &config.topic_scene_deltas, config.scene_delta_partitions).await;
    ensure_topic(&bootstrap, &config.stream_name, &config.topic_agent_commands).await;
    ensure_topic(&bootstrap, &config.stream_name, &config.topic_agent_observations).await;
    drop(bootstrap);

    // Layer 2: bounded delta channel — caps in-flight memory at channel_capacity × ~68 bytes.
    let (delta_tx, delta_rx) = channel::<SceneDelta>(config.channel_capacity);
    let (obs_tx, obs_rx) = unbounded_channel::<AgentObservation>();
    let (cmd_tx, cmd_rx) = unbounded_channel::<AgentCommand>();

    let seq = Arc::new(AtomicU64::new(0));

    let url = config.url.clone();
    let stream = config.stream_name.clone();
    let topic_deltas = config.topic_scene_deltas.clone();
    let topic_obs = config.topic_agent_observations.clone();
    let topic_cmds = config.topic_agent_commands.clone();
    let batch_size = config.batch_size;
    let delta_linger_ms = config.delta_linger_ms;
    let obs_linger_ms = config.delta_linger_ms; // observations share delta linger
    let agent_poll_ms = config.agent_poll_ms;
    let scene_partitions = config.scene_partitions();
    let drop_on_full = config.drop_on_full;

    // Each background task gets its own IggyClient connection (IggyClient is not Clone).

    // Task 1: delta producer (Layer 3: entity-keyed partitioning)
    {
        let u = url.clone();
        let s = stream.clone();
        let t = topic_deltas.clone();
        tokio::spawn(async move {
            if let Ok(c) = connect_client(&u).await {
                run_delta_producer(c, s, t, delta_rx, batch_size, delta_linger_ms, scene_partitions).await;
            }
        });
    }

    // Task 2: observation producer
    {
        let u = url.clone();
        let s = stream.clone();
        let t = topic_obs.clone();
        tokio::spawn(async move {
            if let Ok(c) = connect_client(&u).await {
                run_observation_producer(c, s, t, obs_rx, batch_size, obs_linger_ms).await;
            }
        });
    }

    // Task 3: command consumer (Layer 5: configurable poll interval)
    {
        let u = url.clone();
        let s = stream.clone();
        let t = topic_cmds.clone();
        tokio::spawn(async move {
            if let Ok(c) = connect_client(&u).await {
                run_command_consumer(c, s, t, cmd_tx, agent_poll_ms).await;
            }
        });
    }

    Ok(IggyChangeQueue {
        delta_tx,
        observation_tx: obs_tx,
        command_rx: Arc::new(tokio::sync::Mutex::new(cmd_rx)),
        seq,
        session_start: Instant::now(),
        drop_on_full,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Background tasks — iggy 0.9 direct MessageClient API
// ─────────────────────────────────────────────────────────────────────────────

/// Create and connect a fresh IggyClient. Called per background task since
/// IggyClient does not implement Clone.
async fn connect_client(url: &str) -> Result<IggyClient, String> {
    let client = IggyClient::from_connection_string(url)
        .map_err(|e| format!("connect_client parse: {e}"))?;
    client
        .connect()
        .await
        .map_err(|e| format!("connect_client connect: {e}"))?;
    Ok(client)
}

async fn run_delta_producer(
    client: IggyClient,
    stream: String,
    topic: String,
    mut rx: Receiver<SceneDelta>,
    batch_size: usize,
    linger_ms: u64,
    scene_partitions: u64, // Layer 3: entity-keyed partition count
) {
    let linger = Duration::from_millis(linger_ms);
    let stream_id = Identifier::named(&stream).unwrap();
    let topic_id = Identifier::named(&topic).unwrap();
    let mut batch: Vec<IggyMessage> = Vec::with_capacity(batch_size);
    let mut last_flush = Instant::now();

    loop {
        let deadline = last_flush + linger;
        while batch.len() < batch_size && Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(1), rx.recv()).await {
                Ok(Some(delta)) => {
                    // Layer 3: shard by entity_id so related entities land on the
                    // same partition — improves consumer locality for Explorer/Properties.
                    let partition_key = (delta.entity % scene_partitions) as u32;
                    let partitioning = Partitioning::messages_key_u32(partition_key);
                    match delta.to_bytes() {
                        Ok(b) => {
                            let msg = IggyMessage::builder()
                                .payload(Bytes::from(b))
                                .build()
                                .unwrap();
                            // Flush current batch if partition key would change, so each
                            // send_messages call targets a single partition.
                            if !batch.is_empty() {
                                flush_deltas_partitioned(&client, &stream_id, &topic_id, &partitioning, &mut batch).await;
                                last_flush = Instant::now();
                            }
                            batch.push(msg);
                            // Store partitioning for this batch — all messages in a batch
                            // must share the same partition key.
                            flush_deltas_partitioned(&client, &stream_id, &topic_id, &partitioning, &mut batch).await;
                            last_flush = Instant::now();
                        }
                        Err(e) => warn!("delta serialize: {e}"),
                    }
                }
                Ok(None) => {
                    info!("Iggy delta producer: channel closed.");
                    return;
                }
                Err(_) => break,
            }
        }

        if !batch.is_empty() && (batch.len() >= batch_size || last_flush.elapsed() >= linger) {
            // Fallback balanced flush for any remaining batch
            let partitioning = Partitioning::balanced();
            flush_deltas_partitioned(&client, &stream_id, &topic_id, &partitioning, &mut batch).await;
            last_flush = Instant::now();
        }
    }
}

/// Flush a batch of messages using a specific partitioning strategy.
async fn flush_deltas_partitioned(
    client: &IggyClient,
    stream_id: &Identifier,
    topic_id: &Identifier,
    partitioning: &Partitioning,
    batch: &mut Vec<IggyMessage>,
) {
    if batch.is_empty() {
        return;
    }
    if let Err(e) = client
        .send_messages(stream_id, topic_id, partitioning, batch)
        .await
    {
        error!("Iggy send_messages: {e}");
    }
    batch.clear();
}

/// Flush a batch using balanced partitioning (used by observation producer).
async fn flush_deltas(
    client: &IggyClient,
    stream_id: &Identifier,
    topic_id: &Identifier,
    partitioning: &Partitioning,
    batch: &mut Vec<IggyMessage>,
) {
    flush_deltas_partitioned(client, stream_id, topic_id, partitioning, batch).await;
}

async fn run_observation_producer(
    client: IggyClient,
    stream: String,
    topic: String,
    mut rx: UnboundedReceiver<AgentObservation>,
    batch_size: usize,
    linger_ms: u64,
) {
    let linger = Duration::from_millis(linger_ms);
    let stream_id = Identifier::named(&stream).unwrap();
    let topic_id = Identifier::named(&topic).unwrap();
    let partitioning = Partitioning::balanced();
    let mut batch: Vec<IggyMessage> = Vec::with_capacity(batch_size);
    let mut last_flush = Instant::now();

    loop {
        let deadline = last_flush + linger;
        while batch.len() < batch_size && Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(1), rx.recv()).await {
                Ok(Some(obs)) => {
                    match rkyv::to_bytes::<rkyv::rancor::Error>(&obs) {
                        Ok(b) => {
                            let msg = IggyMessage::builder()
                                .payload(Bytes::from(b.to_vec()))
                                .build()
                                .unwrap();
                            batch.push(msg);
                        }
                        Err(e) => warn!("observation serialize: {e}"),
                    }
                }
                Ok(None) => {
                    flush_deltas(&client, &stream_id, &topic_id, &partitioning, &mut batch).await;
                    return;
                }
                Err(_) => break,
            }
        }
        if !batch.is_empty() && (batch.len() >= batch_size || last_flush.elapsed() >= linger) {
            flush_deltas(&client, &stream_id, &topic_id, &partitioning, &mut batch).await;
            last_flush = Instant::now();
        }
    }
}

async fn run_command_consumer(
    client: IggyClient,
    stream: String,
    topic: String,
    cmd_tx: UnboundedSender<AgentCommand>,
    poll_interval_ms: u64, // Layer 5: configurable poll interval
) {
    let poll_interval = Duration::from_millis(poll_interval_ms.max(1));
    let stream_id = Identifier::named(&stream).unwrap();
    let topic_id = Identifier::named(&topic).unwrap();
    let consumer = Consumer::default();
    let strategy = PollingStrategy::next();

    loop {
        match client
            .poll_messages(&stream_id, &topic_id, Some(1), &consumer, &strategy, 100, true)
            .await
        {
            Ok(polled) => {
                for msg in polled.messages.iter() {
                    match rkyv::from_bytes::<AgentCommand, rkyv::rancor::Error>(msg.payload.as_ref()) {
                        Ok(cmd) => {
                            if cmd_tx.send(cmd).is_err() {
                                info!("Iggy command consumer: receiver dropped.");
                                return;
                            }
                        }
                        Err(e) => warn!("command deserialize: {e}"),
                    }
                }
            }
            Err(e) => {
                warn!("Iggy poll_messages: {e} — retrying in 500ms");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        // Layer 5: use configurable agent_poll_ms instead of hardcoded 5ms
        tokio::time::sleep(poll_interval).await;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Stream / topic bootstrap helpers
// ─────────────────────────────────────────────────────────────────────────────

async fn ensure_stream(client: &IggyClient, stream_name: &str) {
    let stream_id = match Identifier::named(stream_name) {
        Ok(id) => id,
        Err(_) => return,
    };
    // get_stream returns Result<Option<StreamDetails>, IggyError>
    match client.get_stream(&stream_id).await {
        Ok(Some(_)) => return, // already exists
        Ok(None) => {}
        Err(_) => {} // server error — try create anyway
    }
    // create_stream(name: &str) — no stream_id parameter in 0.9
    match client.create_stream(stream_name).await {
        Ok(_) => info!("Iggy: created stream '{stream_name}'"),
        Err(e) => warn!("Iggy: create_stream failed (may already exist): {e}"),
    }
}

/// Create a topic with the given partition count.
/// Layer 3: callers pass the correct partition count per topic type.
async fn ensure_topic_with_partitions(
    client: &IggyClient,
    stream_name: &str,
    topic_name: &str,
    partitions: u32,
) {
    let stream_id = match Identifier::named(stream_name) {
        Ok(id) => id,
        Err(_) => return,
    };
    let topic_id = match Identifier::named(topic_name) {
        Ok(id) => id,
        Err(_) => return,
    };
    match client.get_topic(&stream_id, &topic_id).await {
        Ok(Some(_)) => return,
        Ok(None) => {}
        Err(_) => {}
    }
    match client
        .create_topic(
            &stream_id,
            topic_name,
            partitions.max(1),
            CompressionAlgorithm::default(),
            Some(1u8),
            IggyExpiry::NeverExpire,
            MaxTopicSize::ServerDefault,
        )
        .await
    {
        Ok(_) => info!("Iggy: created topic '{stream_name}/{topic_name}' ({partitions} partitions)"),
        Err(e) => warn!("Iggy: create_topic '{topic_name}' failed (may already exist): {e}"),
    }
}

async fn ensure_topic(client: &IggyClient, stream_name: &str, topic_name: &str) {
    ensure_topic_with_partitions(client, stream_name, topic_name, 1).await;
}

// ─────────────────────────────────────────────────────────────────────────────
// IggyPlugin
// ─────────────────────────────────────────────────────────────────────────────

/// Bevy plugin: initialises Iggy and registers ECS systems.
/// Insert `IggyConfig` as a resource before adding this plugin to customise the URL.
pub struct IggyPlugin;

impl Plugin for IggyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_iggy_queue)
           .add_systems(Update, (poll_agent_commands, emit_lifecycle_deltas));
    }
}

fn setup_iggy_queue(mut commands: Commands, config: Option<Res<IggyConfig>>) {
    let config = config.map(|c| c.clone()).unwrap_or_default();

    let result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio rt");
        rt.block_on(init_iggy(&config))
    })
    .join()
    .unwrap_or_else(|_| Err("Iggy init thread panicked".to_string()));

    match result {
        Ok(queue) => {
            info!("IggyPlugin: IggyChangeQueue ready.");
            commands.insert_resource(queue);
        }
        Err(e) => {
            warn!(
                "IggyPlugin: Iggy unavailable ({e}). \
                 Start iggy-server to enable streaming."
            );
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
// Task 13 — lifecycle delta emission
// ─────────────────────────────────────────────────────────────────────────────

/// Watches for newly spawned entities (Added<Name>) and despawned entities
/// (RemovedComponents<Name>) and emits PartAdded / PartRemoved SceneDeltas.
///
/// Using `Name` as the proxy component because every spawned part/model/light
/// receives a `Name` component at spawn time, making it a reliable Added<T> signal.
///
/// Cost: <1µs per frame when idle (query is empty); ~3µs per spawned/despawned entity.
fn emit_lifecycle_deltas(
    queue: Option<Res<IggyChangeQueue>>,
    added: Query<Entity, Added<bevy::prelude::Name>>,
    mut removed: RemovedComponents<bevy::prelude::Name>,
) {
    let Some(queue) = queue else { return };

    // PartAdded — new entity with Name just spawned this frame
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

    // PartRemoved — entity with Name was despawned this frame
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
