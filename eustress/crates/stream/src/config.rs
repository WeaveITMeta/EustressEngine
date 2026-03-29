//! StreamConfig — tuning knobs for the embedded streaming core.

use std::path::PathBuf;

/// Top-level configuration for an `EustressStream` instance.
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Root directory for segment files.
    /// `None` → in-memory only (no persistence).
    pub data_dir: Option<PathBuf>,

    /// Number of slots in each topic's in-memory ring buffer.
    /// Must be a power of two.  Default: 65 536.
    pub ring_capacity: usize,

    /// Maximum bytes per segment file before a new segment is opened.
    /// Default: 64 MiB.
    pub segment_size: usize,

    /// How many bytes to batch before flushing to storage.
    /// `0` → unbatched (flush on every write).
    /// Default: 256 KiB.
    pub write_buffer_size: usize,

    /// Compress segment data with zstd.
    /// Only active when the `compression` feature is enabled.
    pub compression: bool,

    /// zstd compression level (1 = fastest, 22 = best, 3 = default).
    pub compression_level: i32,

    /// Maximum number of subscriber callbacks per topic.
    /// Extra registrations beyond this limit are silently dropped.
    pub max_subscribers: usize,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            data_dir: None,
            ring_capacity: 65_536,
            segment_size: 64 * 1024 * 1024,   // 64 MiB
            write_buffer_size: 256 * 1024,      // 256 KiB
            compression: true,
            compression_level: 3,
            max_subscribers: 64,
        }
    }
}

impl StreamConfig {
    /// Builder: set persistence directory.
    pub fn with_data_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.data_dir = Some(dir.into());
        self
    }

    /// Builder: set ring buffer capacity (must be power of two).
    pub fn with_ring_capacity(mut self, cap: usize) -> Self {
        assert!(cap.is_power_of_two(), "ring_capacity must be a power of two");
        self.ring_capacity = cap;
        self
    }

    /// Builder: in-memory only (no segment files written).
    pub fn in_memory(mut self) -> Self {
        self.data_dir = None;
        self
    }
}
