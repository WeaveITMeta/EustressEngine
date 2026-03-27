//! # Simulation Stream — Iggy Read/Write for Simulation Records
//!
//! ## Table of Contents
//! - SimStreamConfig        — type alias → `IggyConfig` (Layer 7: single source of truth)
//! - SimStreamWriter        — async writer: publish SimRecord / IterationRecord /
//!                            RuneScriptRecord / WorkshopIterationRecord to Iggy
//! - SimStreamReader        — async reader: replay / query records from Iggy history
//! - bootstrap_sim_topics   — idempotent topic creation with per-topic partition counts
//! - SimQuery               — filter type for reader queries
//!
//! ## Architecture
//!
//! ```text
//! Bevy Resource: Arc<SimStreamWriter>  — one persistent connection, reused across calls
//!
//! run_simulation()          → writer.publish_sim_result()   (no TCP reconnect)
//! process_feedback()        → writer.publish_iteration()    (no TCP reconnect)
//! execute_and_apply()       → writer.publish_rune_script()  (no TCP reconnect)
//! workshop optimize cycle   → writer.publish_workshop_iteration()
//!
//! CLI: eustress sim replay  → SimStreamReader::replay_sim_results()
//! CLI: eustress sim best    → SimStreamReader::best_iteration()
//! Studio convergence panel  → SimStreamReader::workshop_convergence()
//! ```
//!
//! ## Feature Gate
//! Compiled only when `iggy-streaming` feature is enabled.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use tracing::{info, warn};

use iggy::clients::client::IggyClient;
use iggy::prelude::{
    CompressionAlgorithm, Consumer, Identifier, IggyExpiry, IggyMessage, MaxTopicSize,
    MessageClient, Partitioning, PollingStrategy, StreamClient, TopicClient,
};

use crate::iggy_delta::{
    IGGY_TOPIC_ARC_EPISODES, IGGY_TOPIC_ITERATION_HISTORY, IGGY_TOPIC_RUNE_SCRIPTS,
    IGGY_TOPIC_SIM_RESULTS, IGGY_TOPIC_WORKSHOP_ITERATIONS,
};
use crate::iggy_queue::IggyConfig;
use crate::sim_record::{ArcEpisodeRecord, IterationRecord, RuneScriptRecord, SimRecord, WorkshopIterationRecord};

// ─────────────────────────────────────────────────────────────────────────────
// SimStreamConfig — Layer 7: type alias to IggyConfig (single source of truth)
// ─────────────────────────────────────────────────────────────────────────────

/// `SimStreamConfig` is a type alias for `IggyConfig`.
///
/// Layer 7: eliminates the duplicate `url` + `stream_name` fields that existed
/// between `SimStreamConfig` and `IggyConfig`. All call sites that previously
/// constructed `SimStreamConfig::default()` now use `IggyConfig::default()`.
pub type SimStreamConfig = IggyConfig;

// ─────────────────────────────────────────────────────────────────────────────
// SimStreamWriter — publish simulation records to Iggy
// ─────────────────────────────────────────────────────────────────────────────

/// Async writer: publishes simulation records to the four Iggy topics.
///
/// One writer per process — create once, reuse across many publish calls.
/// Not Clone — wrap in Arc if sharing across tasks.
pub struct SimStreamWriter {
    client: IggyClient,
    stream_id: Identifier,
}

impl SimStreamWriter {
    /// Connect to Iggy and ensure all simulation topics exist.
    ///
    /// Layer 1: call this **once** at app startup and store the result as
    /// `Arc<SimStreamWriter>` in Bevy Resources. Never call `connect()` per
    /// `run_simulation()` / `execute_and_apply()` — that costs 50–200ms per call.
    pub async fn connect(config: &SimStreamConfig) -> Result<Self, String> {
        let client = IggyClient::from_connection_string(&config.url)
            .map_err(|e| format!("SimStreamWriter: bad URL: {e}"))?;

        client
            .connect()
            .await
            .map_err(|e| format!("SimStreamWriter: connect failed: {e}"))?;

        let stream_id = Identifier::named(&config.stream_name)
            .map_err(|e| format!("SimStreamWriter: bad stream name: {e}"))?;

        // Layer 8: bootstrap topics with per-type partition counts from config.
        bootstrap_sim_topics(&client, &config.stream_name, config.sim_partitions()).await;

        info!("SimStreamWriter: connected to {}", config.url);
        Ok(Self { client, stream_id })
    }

    /// Wrap in `Arc` for sharing across Bevy systems and async tasks.
    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// Publish a `SimRecord` (one `run_simulation()` result) to `eustress/sim_results`.
    ///
    /// Layer 3: shards by `scenario_id` for consumer locality.
    pub async fn publish_sim_result(&self, record: &SimRecord) -> Result<(), String> {
        let bytes = record.to_bytes().map_err(|e| format!("rkyv SimRecord: {e}"))?;
        let part = Partitioning::messages_key_u32(record.scenario_id as u32);
        self.send_keyed(IGGY_TOPIC_SIM_RESULTS, bytes, part).await
    }

    /// Publish an `IterationRecord` (one VIGA feedback cycle) to `eustress/iteration_history`.
    pub async fn publish_iteration(&self, record: &IterationRecord) -> Result<(), String> {
        let bytes = record.to_bytes().map_err(|e| format!("rkyv IterationRecord: {e}"))?;
        let part = Partitioning::messages_key_u32((record.session_id % 2) as u32);
        self.send_keyed(IGGY_TOPIC_ITERATION_HISTORY, bytes, part).await
    }

    /// Publish a `RuneScriptRecord` (one Rune `execute_and_apply()`) to `eustress/rune_scripts`.
    ///
    /// Layer 3: shards by `scenario_id` — co-locates with sim_results for replay joins.
    pub async fn publish_rune_script(&self, record: &RuneScriptRecord) -> Result<(), String> {
        let bytes = record.to_bytes().map_err(|e| format!("rkyv RuneScriptRecord: {e}"))?;
        let part = Partitioning::messages_key_u32(record.scenario_id as u32);
        self.send_keyed(IGGY_TOPIC_RUNE_SCRIPTS, bytes, part).await
    }

    /// Publish an `ArcEpisodeRecord` (one complete ARC-AGI-3 episode) to `eustress/arc_episodes`.
    ///
    /// Sharded by `session_id` — co-locates all episodes from the same session.
    pub async fn publish_arc_episode(&self, record: &ArcEpisodeRecord) -> Result<(), String> {
        let bytes = record.to_bytes().map_err(|e| format!("rkyv ArcEpisodeRecord: {e}"))?;
        let part = Partitioning::messages_key_u32((record.session_id % 2) as u32);
        self.send_keyed(IGGY_TOPIC_ARC_EPISODES, bytes, part).await
    }

    /// Publish a `WorkshopIterationRecord` to `eustress/workshop_iterations`.
    pub async fn publish_workshop_iteration(
        &self,
        record: &WorkshopIterationRecord,
    ) -> Result<(), String> {
        let bytes = record
            .to_bytes()
            .map_err(|e| format!("rkyv WorkshopIterationRecord: {e}"))?;
        self.send(IGGY_TOPIC_WORKSHOP_ITERATIONS, bytes).await
    }

    // ── internal send helpers ─────────────────────────────────────────────────

    /// Send a single record payload to `topic` using balanced partitioning.
    async fn send(&self, topic: &str, payload: Vec<u8>) -> Result<(), String> {
        self.send_keyed(topic, payload, Partitioning::balanced()).await
    }

    /// Send a single record payload to `topic` with an explicit partition key.
    /// Layer 3: sim_result / rune_script topics shard by scenario_id.
    async fn send_keyed(
        &self,
        topic: &str,
        payload: Vec<u8>,
        partitioning: Partitioning,
    ) -> Result<(), String> {
        let topic_id =
            Identifier::named(topic).map_err(|e| format!("bad topic name '{topic}': {e}"))?;
        let mut msgs = vec![IggyMessage::builder()
            .payload(Bytes::from(payload))
            .build()
            .map_err(|e| format!("IggyMessage build: {e}"))?];

        self.client
            .send_messages(&self.stream_id, &topic_id, &partitioning, &mut msgs)
            .await
            .map_err(|e| format!("send_messages to '{topic}': {e}"))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SimQuery — filter parameters for reader queries
// ─────────────────────────────────────────────────────────────────────────────

/// Filter parameters for querying simulation history.
#[derive(Debug, Clone, Default)]
pub struct SimQuery {
    /// Only return records matching this scenario UUID (hi, lo).
    pub scenario_id: Option<(u64, u64)>,
    /// Only return records matching this product UUID (hi, lo).
    pub product_id: Option<(u64, u64)>,
    /// Only return records matching this session UUID (hi, lo).
    pub session_id: Option<(u64, u64)>,
    /// Maximum records to return (0 = all available).
    pub limit: u32,
    /// Starting Iggy offset (0 = from beginning).
    pub from_offset: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// SimStreamReader — query / replay records from Iggy history
// ─────────────────────────────────────────────────────────────────────────────

/// Async reader: polls Iggy history topics to replay and query simulation records.
///
/// Not Clone — wrap in Arc if sharing across tasks.
pub struct SimStreamReader {
    client: IggyClient,
    stream_id: Identifier,
}

impl SimStreamReader {
    /// Connect to Iggy (read-only — does not bootstrap topics).
    pub async fn connect(config: &SimStreamConfig) -> Result<Self, String> {
        let client = IggyClient::from_connection_string(&config.url)
            .map_err(|e| format!("SimStreamReader: bad URL: {e}"))?;

        client
            .connect()
            .await
            .map_err(|e| format!("SimStreamReader: connect failed: {e}"))?;

        let stream_id = Identifier::named(&config.stream_name)
            .map_err(|e| format!("SimStreamReader: bad stream name: {e}"))?;

        Ok(Self { client, stream_id })
    }

    /// Replay all `SimRecord`s from the Iggy log, optionally filtered by `SimQuery`.
    ///
    /// Returns records in append order (oldest first).
    /// Use this to reconstruct scenario state without touching the file system.
    pub async fn replay_sim_results(&self, query: &SimQuery) -> Vec<SimRecord> {
        let raw = self
            .poll_all(IGGY_TOPIC_SIM_RESULTS, query.from_offset, query.limit)
            .await;

        let mut records: Vec<SimRecord> = raw
            .iter()
            .filter_map(|b| SimRecord::from_bytes(b.as_ref()).ok())
            .filter(|r| {
                if let Some((hi, lo)) = query.scenario_id {
                    let id_hi = (r.scenario_id >> 64) as u64;
                    let id_lo = r.scenario_id as u64;
                    if id_hi != hi || id_lo != lo { return false; }
                }
                true
            })
            .collect();

        records.sort_by_key(|r| r.session_seq);
        records
    }

    /// Return all `IterationRecord`s, optionally filtered by session and scenario.
    pub async fn replay_iterations(&self, query: &SimQuery) -> Vec<IterationRecord> {
        let raw = self
            .poll_all(IGGY_TOPIC_ITERATION_HISTORY, query.from_offset, query.limit)
            .await;

        let mut records: Vec<IterationRecord> = raw
            .iter()
            .filter_map(|b| IterationRecord::from_bytes(b.as_ref()).ok())
            .filter(|r| {
                if let Some((hi, lo)) = query.session_id {
                    let id_hi = (r.session_id >> 64) as u64;
                    let id_lo = r.session_id as u64;
                    if id_hi != hi || id_lo != lo { return false; }
                }
                true
            })
            .collect();

        records.sort_by_key(|r| (r.session_id, r.iteration as u64));
        records
    }

    /// Return the single `IterationRecord` with the highest similarity score
    /// across all sessions (optionally filtered by `query.session_id`).
    pub async fn best_iteration(&self, query: &SimQuery) -> Option<IterationRecord> {
        let records = self.replay_iterations(query).await;
        records
            .into_iter()
            .max_by(|a, b| a.similarity.partial_cmp(&b.similarity).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Return all `RuneScriptRecord`s for a scenario.
    pub async fn replay_rune_scripts(&self, query: &SimQuery) -> Vec<RuneScriptRecord> {
        let raw = self
            .poll_all(IGGY_TOPIC_RUNE_SCRIPTS, query.from_offset, query.limit)
            .await;

        let mut records: Vec<RuneScriptRecord> = raw
            .iter()
            .filter_map(|b| RuneScriptRecord::from_bytes(b.as_ref()).ok())
            .filter(|r| {
                if let Some((hi, lo)) = query.scenario_id {
                    let id_hi = (r.scenario_id >> 64) as u64;
                    let id_lo = r.scenario_id as u64;
                    if id_hi != hi || id_lo != lo { return false; }
                }
                true
            })
            .collect();

        records.sort_by_key(|r| r.session_seq);
        records
    }

    /// Return workshop iteration history for a product, sorted by generation number.
    /// This is the primary data source for the Studio convergence panel.
    pub async fn workshop_convergence(&self, query: &SimQuery) -> Vec<WorkshopIterationRecord> {
        let raw = self
            .poll_all(IGGY_TOPIC_WORKSHOP_ITERATIONS, query.from_offset, query.limit)
            .await;

        let mut records: Vec<WorkshopIterationRecord> = raw
            .iter()
            .filter_map(|b| WorkshopIterationRecord::from_bytes(b.as_ref()).ok())
            .filter(|r| {
                if let Some((hi, lo)) = query.product_id {
                    let id_hi = (r.product_id >> 64) as u64;
                    let id_lo = r.product_id as u64;
                    if id_hi != hi || id_lo != lo { return false; }
                }
                true
            })
            .collect();

        records.sort_by_key(|r| r.generation);
        records
    }

    /// Return all `ArcEpisodeRecord`s from the Iggy log, sorted by `completed_at_ms`.
    pub async fn replay_arc_episodes(&self, query: &SimQuery) -> Vec<ArcEpisodeRecord> {
        let raw = self.poll_all(IGGY_TOPIC_ARC_EPISODES, query.from_offset, query.limit).await;
        let mut records: Vec<ArcEpisodeRecord> = raw
            .iter()
            .filter_map(|b| ArcEpisodeRecord::from_bytes(b.as_ref()).ok())
            .collect();
        records.sort_by_key(|r| r.completed_at_ms);
        records
    }

    /// Return the `ArcEpisodeRecord` with the lowest `efficiency_ratio` for a given `task_id`.
    pub async fn best_arc_episode(&self, task_id: &str) -> Option<ArcEpisodeRecord> {
        let query = SimQuery { limit: 0, ..Default::default() };
        let records = self.replay_arc_episodes(&query).await;
        records
            .into_iter()
            .filter(|r| r.task_id == task_id)
            .min_by(|a, b| {
                a.efficiency_ratio
                    .partial_cmp(&b.efficiency_ratio)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Return the best `WorkshopIterationRecord` (highest fitness) for a product.
    pub async fn best_workshop_generation(
        &self,
        query: &SimQuery,
    ) -> Option<WorkshopIterationRecord> {
        let records = self.workshop_convergence(query).await;
        records
            .into_iter()
            .max_by(|a, b| a.fitness.partial_cmp(&b.fitness).unwrap_or(std::cmp::Ordering::Equal))
    }

    // ── internal poll helper ──────────────────────────────────────────────────

    /// Poll up to `limit` (or 1000 per page if 0) messages from a topic starting at `offset`.
    ///
    /// Layer 4: returns `Vec<Bytes>` — each `Bytes` is an `Arc<[u8]>` clone (O(1),
    /// no heap copy). Callers pass `b.as_ref()` directly to rkyv `from_bytes(&[u8])`.
    async fn poll_all(&self, topic: &str, offset: u64, limit: u32) -> Vec<Bytes> {
        let topic_id = match Identifier::named(topic) {
            Ok(id) => id,
            Err(e) => {
                warn!("SimStreamReader: bad topic '{topic}': {e}");
                return vec![];
            }
        };

        let count = if limit == 0 { 1000 } else { limit };
        let consumer = Consumer::default();

        // Paginate — Iggy caps count per poll at ~64K messages; loop until exhausted.
        let mut all: Vec<Bytes> = Vec::new();
        let mut current_offset = offset;
        let mut remaining = if limit == 0 { u64::MAX } else { limit as u64 };

        loop {
            let batch_count = remaining.min(count as u64) as u32;
            let strat = PollingStrategy::offset(current_offset);

            match self
                .client
                .poll_messages(
                    &self.stream_id,
                    &topic_id,
                    Some(1),
                    &consumer,
                    &strat,
                    batch_count,
                    false, // do not auto-commit — reads are idempotent
                )
                .await
            {
                Ok(polled) => {
                    if polled.messages.is_empty() {
                        break;
                    }
                    let fetched = polled.messages.len() as u64;
                    for msg in polled.messages.iter() {
                        // Layer 4: clone Bytes (Arc<[u8]> refcount bump, O(1), zero copy)
                        all.push(msg.payload.clone());
                    }
                    current_offset += fetched;
                    if let Some(r) = remaining.checked_sub(fetched) {
                        remaining = r;
                    } else {
                        break;
                    }
                    if fetched < batch_count as u64 {
                        break; // End of topic
                    }
                }
                Err(e) => {
                    warn!("SimStreamReader: poll '{topic}' @ {current_offset}: {e}");
                    break;
                }
            }
        }

        all
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// bootstrap_sim_topics — idempotent topic creation
// ─────────────────────────────────────────────────────────────────────────────

/// Ensure all four simulation topics exist in the Iggy stream.
///
/// Layer 8: `sim_result_partitions` sets partition count on the high-write topics;
/// `iteration_history` and `workshop_iterations` use 2 partitions (session-sharded).
/// Called by `SimStreamWriter::connect()` — safe to call multiple times (idempotent).
pub async fn bootstrap_sim_topics(client: &IggyClient, stream_name: &str, sim_partitions: u32) {
    let stream_id = match Identifier::named(stream_name) {
        Ok(id) => id,
        Err(_) => return,
    };

    // Ensure the stream itself exists first.
    match client.get_stream(&stream_id).await {
        Ok(Some(_)) => {}
        Ok(None) | Err(_) => {
            let _ = client.create_stream(stream_name).await;
        }
    }

    // (topic, partition_count) — sim/rune use sim_partitions, history/workshop/arc use 2.
    let topics: &[(&str, u32)] = &[
        (IGGY_TOPIC_SIM_RESULTS,        sim_partitions.max(1)),
        (IGGY_TOPIC_ITERATION_HISTORY,  2),
        (IGGY_TOPIC_RUNE_SCRIPTS,       sim_partitions.max(1)),
        (IGGY_TOPIC_WORKSHOP_ITERATIONS, 2),
        (IGGY_TOPIC_ARC_EPISODES,       2),
    ];

    for (topic, partitions) in topics {
        let topic_id = match Identifier::named(topic) {
            Ok(id) => id,
            Err(_) => continue,
        };

        match client.get_topic(&stream_id, &topic_id).await {
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => {}
        }

        match client
            .create_topic(
                &stream_id,
                topic,
                *partitions,
                CompressionAlgorithm::default(),
                Some(1u8),
                IggyExpiry::NeverExpire,
                MaxTopicSize::ServerDefault,
            )
            .await
        {
            Ok(_) => info!("SimStream: created topic '{stream_name}/{topic}' ({partitions} partitions)"),
            Err(e) => warn!("SimStream: create_topic '{topic}' (may exist): {e}"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Synchronous fire-and-forget helpers
// ─────────────────────────────────────────────────────────────────────────────
//
// Layer 1 fix: these helpers now accept an `Arc<SimStreamWriter>` that was
// connected once at app startup (stored as a Bevy Resource). This eliminates
// the 50–200ms TCP reconnect that previously happened on every call.
//
// Fallback: if no writer is provided (`None`), a one-shot connection is made
// (original behaviour). This keeps the call sites backward-compatible during
// the transition period before the Resource is wired up.

/// Fire-and-forget: publish a `SimRecord`.
///
/// Preferred: pass `Some(writer)` from the `Arc<SimStreamWriter>` Bevy Resource.
/// Fallback: pass `None` + `config` to connect on-demand (legacy, ~50–200ms overhead).
pub fn publish_sim_result_sync(
    writer: Option<Arc<SimStreamWriter>>,
    config: SimStreamConfig,
    record: SimRecord,
) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            let w = match writer {
                Some(w) => w,
                None => match SimStreamWriter::connect(&config).await {
                    Ok(w) => Arc::new(w),
                    Err(e) => { warn!("publish_sim_result_sync connect: {e}"); return; }
                },
            };
            if let Err(e) = w.publish_sim_result(&record).await {
                warn!("publish_sim_result_sync: {e}");
            }
        });
    }
}

/// Fire-and-forget: publish an `IterationRecord`.
pub fn publish_iteration_sync(
    writer: Option<Arc<SimStreamWriter>>,
    config: SimStreamConfig,
    record: IterationRecord,
) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            let w = match writer {
                Some(w) => w,
                None => match SimStreamWriter::connect(&config).await {
                    Ok(w) => Arc::new(w),
                    Err(e) => { warn!("publish_iteration_sync connect: {e}"); return; }
                },
            };
            if let Err(e) = w.publish_iteration(&record).await {
                warn!("publish_iteration_sync: {e}");
            }
        });
    }
}

/// Fire-and-forget: publish a `RuneScriptRecord`.
pub fn publish_rune_script_sync(
    writer: Option<Arc<SimStreamWriter>>,
    config: SimStreamConfig,
    record: RuneScriptRecord,
) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            let w = match writer {
                Some(w) => w,
                None => match SimStreamWriter::connect(&config).await {
                    Ok(w) => Arc::new(w),
                    Err(e) => { warn!("publish_rune_script_sync connect: {e}"); return; }
                },
            };
            if let Err(e) = w.publish_rune_script(&record).await {
                warn!("publish_rune_script_sync: {e}");
            }
        });
    }
}

/// Fire-and-forget: publish a `WorkshopIterationRecord`.
pub fn publish_workshop_iteration_sync(
    writer: Option<Arc<SimStreamWriter>>,
    config: SimStreamConfig,
    record: WorkshopIterationRecord,
) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            let w = match writer {
                Some(w) => w,
                None => match SimStreamWriter::connect(&config).await {
                    Ok(w) => Arc::new(w),
                    Err(e) => { warn!("publish_workshop_iteration_sync connect: {e}"); return; }
                },
            };
            if let Err(e) = w.publish_workshop_iteration(&record).await {
                warn!("publish_workshop_iteration_sync: {e}");
            }
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Utility
// ─────────────────────────────────────────────────────────────────────────────

/// Current Unix timestamp in milliseconds.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
