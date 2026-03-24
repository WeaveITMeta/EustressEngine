//! # Dirty-Bit Write-Back Flusher
//!
//! ## Table of Contents
//! - DirtyBitFlusher   — background thread that batches dirty records to disk
//! - FlushStats        — per-pass counters for telemetry
//!
//! ## Design
//! On any in-memory mutation (ECS component change, hot-cache update):
//!   1. InstanceRecord.dirty = true        — immediate (1 atomic store)
//!   2. InstanceRecord.version += 1        — immediate (1 atomic add)
//!   3. Background flusher wakes every `flush_interval` (default 100ms)
//!   4. Collects all dirty IDs from the SpatialChunkGrid
//!   5. Batches up to `flush_batch_size` (default 1000) per pass
//!   6. For each: serialize TOML to disk + re-encode .bin sidecar
//!   7. Mark record as flushed (dirty = false)
//!
//! The hot path (ECS mutation) never blocks on disk I/O.
//! Worst-case latency from edit to disk: `flush_interval` + I/O time.
//!
//! ## Benchmark-Proven Numbers
//! - Sidecar encode: ~41K instances/sec (bincode + zstd level 1)
//! - At batch_size=1000 and 41K/s encode rate: ~24ms per batch

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use tracing;

use super::chunk_grid::SpatialChunkGrid;
use super::sidecar;
use super::types::StreamingConfig;

// ─────────────────────────────────────────────────────────────────────────────
// FlushStats — per-pass telemetry
// ─────────────────────────────────────────────────────────────────────────────

/// Counters from one flush pass.
#[derive(Debug, Clone, Default)]
pub struct FlushStats {
    /// Number of dirty records found this pass.
    pub dirty_found: usize,
    /// Number of records successfully flushed to disk.
    pub flushed: usize,
    /// Number of records that failed to flush (I/O errors).
    pub failed: usize,
    /// Wall time for this flush pass.
    pub elapsed: std::time::Duration,
}

// ─────────────────────────────────────────────────────────────────────────────
// DirtyBitFlusher
// ─────────────────────────────────────────────────────────────────────────────

/// Background write-back thread that flushes dirty instance records to disk.
///
/// Runs in a dedicated OS thread (not async — disk I/O is the bottleneck,
/// and we want predictable latency without executor contention).
pub struct DirtyBitFlusher {
    /// Signal to stop the background thread.
    shutdown: Arc<AtomicBool>,
    /// Join handle for the background thread.
    handle: Option<thread::JoinHandle<()>>,
}

impl DirtyBitFlusher {
    /// Start the flusher background thread.
    ///
    /// # Arguments
    /// - `grid` — shared reference to the SpatialChunkGrid (Arc for thread safety)
    /// - `config` — streaming config (flush_interval, flush_batch_size)
    pub fn start(grid: Arc<SpatialChunkGrid>, config: StreamingConfig) -> Self {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        let handle = thread::Builder::new()
            .name("eustress-dirty-flusher".into())
            .spawn(move || {
                tracing::info!(
                    "DirtyBitFlusher started — interval={:?}, batch_size={}",
                    config.flush_interval, config.flush_batch_size
                );

                while !shutdown_clone.load(Ordering::Relaxed) {
                    thread::sleep(config.flush_interval);

                    if shutdown_clone.load(Ordering::Relaxed) {
                        break;
                    }

                    let stats = flush_pass(&grid, config.flush_batch_size);

                    if stats.flushed > 0 || stats.failed > 0 {
                        tracing::debug!(
                            "DirtyBitFlusher: flushed={} failed={} elapsed={:?}",
                            stats.flushed, stats.failed, stats.elapsed
                        );
                    }
                }

                // Final flush on shutdown — write any remaining dirty records.
                tracing::info!("DirtyBitFlusher shutting down — final flush");
                let stats = flush_pass(&grid, usize::MAX);
                if stats.flushed > 0 {
                    tracing::info!(
                        "DirtyBitFlusher final flush: {} records in {:?}",
                        stats.flushed, stats.elapsed
                    );
                }
            })
            .expect("failed to spawn dirty-flusher thread");

        Self {
            shutdown,
            handle: Some(handle),
        }
    }

    /// Signal the flusher to stop and wait for it to finish.
    /// Performs a final flush of all remaining dirty records before returning.
    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    /// Check if the flusher is still running.
    pub fn is_running(&self) -> bool {
        self.handle.as_ref().map_or(false, |h| !h.is_finished())
    }
}

impl Drop for DirtyBitFlusher {
    fn drop(&mut self) {
        self.stop();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// flush_pass — one batch of dirty record writes
// ─────────────────────────────────────────────────────────────────────────────

/// Execute one flush pass: collect dirty IDs, write TOML + sidecar, mark clean.
fn flush_pass(grid: &SpatialChunkGrid, batch_limit: usize) -> FlushStats {
    let t0 = Instant::now();

    let dirty_ids = grid.collect_dirty_ids();
    let dirty_found = dirty_ids.len();

    if dirty_found == 0 {
        return FlushStats {
            dirty_found: 0,
            flushed: 0,
            failed: 0,
            elapsed: t0.elapsed(),
        };
    }

    let mut flushed = 0usize;
    let mut failed = 0usize;

    for id in dirty_ids.into_iter().take(batch_limit) {
        let success = grid.with_instance(&id, |record| {
            // Write canonical TOML file.
            if let Err(error) = write_toml(record) {
                tracing::warn!("flush TOML failed for {}: {error}", record.id);
                return false;
            }

            // Re-encode .bin sidecar.
            if let Err(error) = sidecar::encode_sidecar(
                &record.sidecar_path,
                &record.bin,
                record.current_version(),
                &record.toml_path,
            ) {
                tracing::warn!("flush sidecar failed for {}: {error}", record.id);
                return false;
            }

            // Mark as flushed (dirty = false).
            record.mark_flushed();
            true
        });

        match success {
            Some(true) => flushed += 1,
            _ => failed += 1,
        }
    }

    FlushStats {
        dirty_found,
        flushed,
        failed,
        elapsed: t0.elapsed(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TOML serialization helper
// ─────────────────────────────────────────────────────────────────────────────

/// Serialize an InstanceRecord back to its canonical TOML file.
/// Uses atomic write (temp file + rename) to prevent torn reads.
fn write_toml(record: &super::types::InstanceRecord) -> Result<(), String> {
    use std::io::Write;

    // Build a minimal TOML representation.
    // This matches the Eustress canonical instance format.
    let toml_content = toml::to_string_pretty(&TomlInstance {
        id:       record.id.as_str().to_string(),
        name:     record.name.clone(),
        tags:     record.tags.clone(),
        position: record.bin.position.to_vec(),
        rotation: record.bin.rotation.to_vec(),
        scale:    record.bin.scale,
        class_id: record.bin.class_id,
        velocity: record.bin.velocity,
    }).map_err(|e| format!("toml serialize: {e}"))?;

    // Atomic write: temp file → rename.
    let tmp_path = record.toml_path.with_extension("toml.tmp");
    let mut file = std::fs::File::create(&tmp_path)
        .map_err(|e| format!("create tmp: {e}"))?;
    file.write_all(toml_content.as_bytes())
        .map_err(|e| format!("write tmp: {e}"))?;
    file.flush()
        .map_err(|e| format!("flush tmp: {e}"))?;
    drop(file);

    std::fs::rename(&tmp_path, &record.toml_path)
        .map_err(|e| format!("rename: {e}"))?;

    Ok(())
}

/// Minimal TOML-serializable instance shape for write-back.
#[derive(serde::Serialize)]
struct TomlInstance {
    id:       String,
    name:     String,
    tags:     Vec<String>,
    position: Vec<f32>,
    rotation: Vec<f32>,
    scale:    f32,
    class_id: u32,
    velocity: f32,
}
