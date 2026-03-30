//! # Simulation Stream — EustressStream Read/Write for Simulation Records
//!
//! Replaces the former Apache Iggy integration with `eustress-stream`.
//! The public API is unchanged so all call sites continue to compile.
//!
//! ## Architecture
//!
//! ```text
//! Bevy Resource: Arc<SimStreamWriter>  — one EustressStream per process
//!
//! run_simulation()          → writer.publish_sim_result()
//! process_feedback()        → writer.publish_iteration()
//! execute_and_apply()       → writer.publish_rune_script()
//! workshop optimize cycle   → writer.publish_workshop_iteration()
//! ```
//!
//! ## Feature Gate
//! Compiled only when `streaming` feature is enabled.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use tracing::info;

use eustress_stream::{EustressStream, StreamConfig};

use crate::scene_delta::{
    TOPIC_ARC_EPISODES, TOPIC_ITERATION_HISTORY, TOPIC_RUNE_SCRIPTS,
    TOPIC_SIM_RESULTS, TOPIC_WORKSHOP_ITERATIONS,
};
use crate::change_queue::ChangeQueueConfig;
use crate::sim_record::{ArcEpisodeRecord, IterationRecord, RuneScriptRecord, SimRecord, WorkshopIterationRecord};

// ─────────────────────────────────────────────────────────────────────────────
// SimStreamConfig — type alias to ChangeQueueConfig
// ─────────────────────────────────────────────────────────────────────────────

pub type SimStreamConfig = ChangeQueueConfig;

// ─────────────────────────────────────────────────────────────────────────────
// SimStreamWriter
// ─────────────────────────────────────────────────────────────────────────────

/// Async writer: publishes simulation records via EustressStream.
///
/// Create once and store as `Arc<SimStreamWriter>` in Bevy Resources.
pub struct SimStreamWriter {
    stream: EustressStream,
}

impl SimStreamWriter {
    /// Initialise with a private in-memory EustressStream.
    ///
    /// Pass `Some(stream)` from `ChangeQueue.stream.clone()` if you need
    /// the writer and reader to share topics within the same process.
    pub fn with_stream(stream: EustressStream) -> Self {
        Self { stream }
    }

    /// API-compatible constructor — ignores `config` URL/stream fields.
    pub async fn connect(_config: &SimStreamConfig) -> Result<Self, String> {
        let stream = EustressStream::new(StreamConfig::default().in_memory());
        info!("SimStreamWriter: EustressStream ready (in-process).");
        Ok(Self { stream })
    }

    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }

    pub async fn publish_sim_result(&self, record: &SimRecord) -> Result<(), String> {
        let bytes = record.to_bytes().map_err(|e| format!("rkyv SimRecord: {e}"))?;
        self.stream.producer(TOPIC_SIM_RESULTS).send_bytes(Bytes::from(bytes));
        Ok(())
    }

    pub async fn publish_iteration(&self, record: &IterationRecord) -> Result<(), String> {
        let bytes = record.to_bytes().map_err(|e| format!("rkyv IterationRecord: {e}"))?;
        self.stream.producer(TOPIC_ITERATION_HISTORY).send_bytes(Bytes::from(bytes));
        Ok(())
    }

    pub async fn publish_rune_script(&self, record: &RuneScriptRecord) -> Result<(), String> {
        let bytes = record.to_bytes().map_err(|e| format!("rkyv RuneScriptRecord: {e}"))?;
        self.stream.producer(TOPIC_RUNE_SCRIPTS).send_bytes(Bytes::from(bytes));
        Ok(())
    }

    pub async fn publish_arc_episode(&self, record: &ArcEpisodeRecord) -> Result<(), String> {
        let bytes = record.to_bytes().map_err(|e| format!("rkyv ArcEpisodeRecord: {e}"))?;
        self.stream.producer(TOPIC_ARC_EPISODES).send_bytes(Bytes::from(bytes));
        Ok(())
    }

    pub async fn publish_workshop_iteration(
        &self,
        record: &WorkshopIterationRecord,
    ) -> Result<(), String> {
        let bytes = record.to_bytes().map_err(|e| format!("rkyv WorkshopIterationRecord: {e}"))?;
        self.stream.producer(TOPIC_WORKSHOP_ITERATIONS).send_bytes(Bytes::from(bytes));
        Ok(())
    }

    /// Access the underlying stream (e.g. to share with a `SimStreamReader`).
    pub fn stream(&self) -> &EustressStream { &self.stream }
}

// ─────────────────────────────────────────────────────────────────────────────
// SimQuery
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct SimQuery {
    pub scenario_id: Option<(u64, u64)>,
    pub product_id: Option<(u64, u64)>,
    pub session_id: Option<(u64, u64)>,
    pub limit: u32,
    pub from_offset: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// SimStreamReader
// ─────────────────────────────────────────────────────────────────────────────

/// Async reader: replays simulation records from the in-memory ring buffer.
pub struct SimStreamReader {
    stream: EustressStream,
}

impl SimStreamReader {
    /// Initialise with a private in-memory EustressStream.
    ///
    /// Pass `Some(stream)` from the writer's `stream()` if you need to replay
    /// records that were written in the same process.
    pub fn with_stream(stream: EustressStream) -> Self {
        Self { stream }
    }

    /// API-compatible constructor — creates a standalone (empty) stream.
    pub async fn connect(_config: &SimStreamConfig) -> Result<Self, String> {
        let stream = EustressStream::new(StreamConfig::default().in_memory());
        Ok(Self { stream })
    }

    pub async fn replay_sim_results(&self, query: &SimQuery) -> Vec<SimRecord> {
        let mut records: Vec<SimRecord> = Vec::new();
        self.stream.replay_ring(TOPIC_SIM_RESULTS, query.from_offset, |view| {
            if let Ok(r) = SimRecord::from_bytes(view.data) {
                if let Some((hi, lo)) = query.scenario_id {
                    let id_hi = (r.scenario_id >> 64) as u64;
                    let id_lo = r.scenario_id as u64;
                    if id_hi != hi || id_lo != lo { return; }
                }
                records.push(r);
            }
        });
        if query.limit > 0 { records.truncate(query.limit as usize); }
        records.sort_by_key(|r| r.session_seq);
        records
    }

    pub async fn replay_iterations(&self, query: &SimQuery) -> Vec<IterationRecord> {
        let mut records: Vec<IterationRecord> = Vec::new();
        self.stream.replay_ring(TOPIC_ITERATION_HISTORY, query.from_offset, |view| {
            if let Ok(r) = IterationRecord::from_bytes(view.data) {
                if let Some((hi, lo)) = query.session_id {
                    let id_hi = (r.session_id >> 64) as u64;
                    let id_lo = r.session_id as u64;
                    if id_hi != hi || id_lo != lo { return; }
                }
                records.push(r);
            }
        });
        if query.limit > 0 { records.truncate(query.limit as usize); }
        records.sort_by_key(|r| (r.session_id, r.iteration as u64));
        records
    }

    pub async fn best_iteration(&self, query: &SimQuery) -> Option<IterationRecord> {
        let records = self.replay_iterations(query).await;
        records
            .into_iter()
            .max_by(|a, b| a.similarity.partial_cmp(&b.similarity).unwrap_or(std::cmp::Ordering::Equal))
    }

    pub async fn replay_rune_scripts(&self, query: &SimQuery) -> Vec<RuneScriptRecord> {
        let mut records: Vec<RuneScriptRecord> = Vec::new();
        self.stream.replay_ring(TOPIC_RUNE_SCRIPTS, query.from_offset, |view| {
            if let Ok(r) = RuneScriptRecord::from_bytes(view.data) {
                if let Some((hi, lo)) = query.scenario_id {
                    let id_hi = (r.scenario_id >> 64) as u64;
                    let id_lo = r.scenario_id as u64;
                    if id_hi != hi || id_lo != lo { return; }
                }
                records.push(r);
            }
        });
        if query.limit > 0 { records.truncate(query.limit as usize); }
        records.sort_by_key(|r| r.session_seq);
        records
    }

    pub async fn workshop_convergence(&self, query: &SimQuery) -> Vec<WorkshopIterationRecord> {
        let mut records: Vec<WorkshopIterationRecord> = Vec::new();
        self.stream.replay_ring(TOPIC_WORKSHOP_ITERATIONS, query.from_offset, |view| {
            if let Ok(r) = WorkshopIterationRecord::from_bytes(view.data) {
                if let Some((hi, lo)) = query.product_id {
                    let id_hi = (r.product_id >> 64) as u64;
                    let id_lo = r.product_id as u64;
                    if id_hi != hi || id_lo != lo { return; }
                }
                records.push(r);
            }
        });
        if query.limit > 0 { records.truncate(query.limit as usize); }
        records.sort_by_key(|r| r.generation);
        records
    }

    pub async fn replay_arc_episodes(&self, query: &SimQuery) -> Vec<ArcEpisodeRecord> {
        let mut records: Vec<ArcEpisodeRecord> = Vec::new();
        self.stream.replay_ring(TOPIC_ARC_EPISODES, query.from_offset, |view| {
            if let Ok(r) = ArcEpisodeRecord::from_bytes(view.data) {
                records.push(r);
            }
        });
        if query.limit > 0 { records.truncate(query.limit as usize); }
        records.sort_by_key(|r| r.completed_at_ms);
        records
    }

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

    pub async fn best_workshop_generation(
        &self,
        query: &SimQuery,
    ) -> Option<WorkshopIterationRecord> {
        let records = self.workshop_convergence(query).await;
        records
            .into_iter()
            .max_by(|a, b| a.fitness.partial_cmp(&b.fitness).unwrap_or(std::cmp::Ordering::Equal))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fire-and-forget sync helpers
// ─────────────────────────────────────────────────────────────────────────────

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
                    Err(e) => { tracing::warn!("publish_sim_result_sync: {e}"); return; }
                },
            };
            if let Err(e) = w.publish_sim_result(&record).await {
                tracing::warn!("publish_sim_result_sync: {e}");
            }
        });
    }
}

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
                    Err(e) => { tracing::warn!("publish_iteration_sync: {e}"); return; }
                },
            };
            if let Err(e) = w.publish_iteration(&record).await {
                tracing::warn!("publish_iteration_sync: {e}");
            }
        });
    }
}

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
                    Err(e) => { tracing::warn!("publish_rune_script_sync: {e}"); return; }
                },
            };
            if let Err(e) = w.publish_rune_script(&record).await {
                tracing::warn!("publish_rune_script_sync: {e}");
            }
        });
    }
}

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
                    Err(e) => { tracing::warn!("publish_workshop_iteration_sync: {e}"); return; }
                },
            };
            if let Err(e) = w.publish_workshop_iteration(&record).await {
                tracing::warn!("publish_workshop_iteration_sync: {e}");
            }
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Utility
// ─────────────────────────────────────────────────────────────────────────────

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
