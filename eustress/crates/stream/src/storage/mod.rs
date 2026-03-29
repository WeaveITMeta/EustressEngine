//! Storage backends — platform-specific I/O for segment log persistence.
//!
//! ## Hierarchy
//! ```text
//! StorageBackend (trait)
//!   ├── NullBackend         — in-memory only, no-op appends
//!   ├── MmapBackend         — cross-platform mmap + tokio::fs fallback
//!   └── IoUringBackend      — Linux io_uring (feature = "io-uring")
//! ```

mod mmap;
mod null;
#[cfg(all(target_os = "linux", feature = "io-uring"))]
mod io_uring;

pub use mmap::MmapBackend;
pub use null::NullBackend;
#[cfg(all(target_os = "linux", feature = "io-uring"))]
pub use io_uring::IoUringBackend;

use std::sync::Arc;
use bytes::Bytes;

use crate::config::StreamConfig;
use crate::error::StreamError;

// ─────────────────────────────────────────────────────────────────────────────
// StorageBackend trait
// ─────────────────────────────────────────────────────────────────────────────

/// Async append-only storage interface.
///
/// Implementations must be `Send + Sync + 'static` so they can be shared
/// across threads via `Arc<dyn StorageBackend>`.
#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync + 'static {
    /// Append a chunk of bytes (header + payload) to the current segment.
    /// Returns the byte offset at which the chunk was written.
    async fn append(&self, data: Bytes) -> Result<u64, StreamError>;

    /// Read `len` bytes starting at `byte_offset`.
    async fn read_range(&self, byte_offset: u64, len: usize) -> Result<Bytes, StreamError>;

    /// Flush all pending writes to durable storage.
    async fn flush(&self) -> Result<(), StreamError>;

    /// Current byte length of the active segment.
    fn byte_len(&self) -> u64;
}

// ─────────────────────────────────────────────────────────────────────────────
// Factory
// ─────────────────────────────────────────────────────────────────────────────

/// Create the best available `StorageBackend` for the current platform and
/// config.  Returns `None` when `config.data_dir` is `None` (in-memory only).
pub fn create_backend(
    config: &StreamConfig,
    topic: &str,
) -> Option<Arc<dyn StorageBackend>> {
    let dir = config.data_dir.as_ref()?;
    let topic_dir = dir.join(topic);
    std::fs::create_dir_all(&topic_dir).ok()?;

    // On Linux with the io-uring feature, prefer the io_uring backend.
    #[cfg(all(target_os = "linux", feature = "io-uring"))]
    {
        match IoUringBackend::open(topic_dir, config.segment_size) {
            Ok(b) => return Some(Arc::new(b)),
            Err(e) => tracing::warn!("io_uring backend unavailable: {e}, falling back to mmap"),
        }
    }

    // Default: cross-platform mmap backend.
    match MmapBackend::open(topic_dir, config.segment_size, config.compression_level) {
        Ok(b)  => Some(Arc::new(b)),
        Err(e) => {
            tracing::error!("Failed to open mmap backend for topic '{topic}': {e}");
            None
        }
    }
}
