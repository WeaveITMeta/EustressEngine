//! MmapBackend — cross-platform segment log using memmap2 + tokio::fs.
//!
//! ## Segment layout
//!
//! ```text
//! topic_dir/
//!   000000000000.log   ← first segment (hex offset of first message)
//!   000000000040.log   ← next segment, etc.
//! ```
//!
//! Within each `.log` file: `[MessageHeader(32 bytes)][payload…][MessageHeader…]…`
//!
//! The active segment is opened with `O_CREAT | O_RDWR`, pre-extended to
//! `segment_size` bytes (fallocate on Linux, seek+write on other platforms),
//! then memory-mapped.  Writes advance a `write_pos` cursor.
//!
//! On segment overflow: the current map is flushed + closed; a new segment
//! file is created with its base offset as the filename.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use memmap2::MmapMut;
use parking_lot::Mutex;

use super::StorageBackend;
use crate::error::StreamError;

// ─────────────────────────────────────────────────────────────────────────────
// Inner state (behind a Mutex — segment roll-over needs exclusive access)
// ─────────────────────────────────────────────────────────────────────────────

struct Inner {
    dir:          PathBuf,
    segment_size: usize,
    compression_level: i32,
    /// Memory-mapped active segment.
    mmap:         MmapMut,
    /// Byte position within the active segment.
    write_pos:    usize,
    /// Base offset of the active segment file (used for naming).
    segment_base: u64,
    /// Total bytes written across all segments.
    total_bytes:  u64,
}

impl Inner {
    fn open(dir: &Path, segment_size: usize, compression_level: i32) -> Result<Self, StreamError> {
        // Find the latest segment or create the first one.
        let (seg_path, seg_base, write_pos) = find_or_create_segment(dir, segment_size)?;
        let mmap = open_mmap(&seg_path, segment_size)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            segment_size,
            compression_level,
            mmap,
            write_pos,
            segment_base: seg_base,
            total_bytes: seg_base + write_pos as u64,
        })
    }

    fn append_bytes(&mut self, data: &[u8]) -> Result<u64, StreamError> {
        let offset = self.total_bytes;

        // Roll to a new segment if this write would overflow.
        if self.write_pos + data.len() > self.segment_size {
            self.roll_segment()?;
        }

        let end = self.write_pos + data.len();
        self.mmap[self.write_pos..end].copy_from_slice(data);
        self.write_pos  += data.len();
        self.total_bytes += data.len() as u64;

        Ok(offset)
    }

    fn roll_segment(&mut self) -> Result<(), StreamError> {
        self.mmap.flush().map_err(|e| StreamError::Io(e.to_string()))?;
        let new_base = self.total_bytes;
        let seg_name = format!("{:016x}.log", new_base);
        let seg_path = self.dir.join(seg_name);
        self.mmap = open_mmap(&seg_path, self.segment_size)?;
        self.segment_base = new_base;
        self.write_pos    = 0;
        Ok(())
    }

    fn read_at(&self, byte_offset: u64, len: usize) -> Result<Bytes, StreamError> {
        // Only in-segment reads for now (active segment).
        let seg_offset = byte_offset.saturating_sub(self.segment_base) as usize;
        if seg_offset + len > self.write_pos {
            return Err(StreamError::OutOfRange { offset: byte_offset, len });
        }
        Ok(Bytes::copy_from_slice(&self.mmap[seg_offset..seg_offset + len]))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MmapBackend
// ─────────────────────────────────────────────────────────────────────────────

pub struct MmapBackend {
    inner: Arc<Mutex<Inner>>,
}

impl MmapBackend {
    pub fn open(dir: PathBuf, segment_size: usize, compression_level: i32) -> Result<Self, StreamError> {
        let inner = Inner::open(&dir, segment_size, compression_level)?;
        Ok(Self { inner: Arc::new(Mutex::new(inner)) })
    }
}

#[async_trait::async_trait]
impl StorageBackend for MmapBackend {
    async fn append(&self, data: Bytes) -> Result<u64, StreamError> {
        let mut inner = self.inner.lock();

        // Optional zstd compression.
        #[cfg(feature = "compression")]
        let payload: Vec<u8> = {
            let lvl = inner.compression_level;
            zstd::encode_all(data.as_ref(), lvl)
                .map_err(|e| StreamError::Compression(e.to_string()))?
        };
        #[cfg(not(feature = "compression"))]
        let payload: &[u8] = &data;

        #[cfg(feature = "compression")]
        let payload_ref: &[u8] = &payload;
        #[cfg(not(feature = "compression"))]
        let payload_ref: &[u8] = payload;

        inner.append_bytes(payload_ref)
    }

    async fn read_range(&self, byte_offset: u64, len: usize) -> Result<Bytes, StreamError> {
        self.inner.lock().read_at(byte_offset, len)
    }

    async fn flush(&self) -> Result<(), StreamError> {
        self.inner.lock().mmap.flush().map_err(|e| StreamError::Io(e.to_string()))
    }

    fn byte_len(&self) -> u64 {
        self.inner.lock().total_bytes
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Public alias used by the io_uring backend.
pub fn find_or_create_segment_pub(
    dir: &std::path::Path,
    segment_size: usize,
) -> Result<(std::path::PathBuf, u64, usize), crate::error::StreamError> {
    find_or_create_segment(dir, segment_size)
}

/// Find the last existing `.log` file in `dir` or create `000000000000.log`.
/// Returns (path, base_offset, write_pos).
fn find_or_create_segment(dir: &Path, segment_size: usize) -> Result<(PathBuf, u64, usize), StreamError> {
    let mut segments: Vec<(u64, PathBuf)> = std::fs::read_dir(dir)
        .map_err(|e| StreamError::Io(e.to_string()))?
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_str()?;
            if name.ends_with(".log") && name.len() == 20 {
                let base = u64::from_str_radix(&name[..16], 16).ok()?;
                Some((base, p))
            } else {
                None
            }
        })
        .collect();

    segments.sort_by_key(|(base, _)| *base);

    match segments.last() {
        Some((base, path)) => {
            // Determine actual write_pos by scanning existing data.
            let write_pos = measure_write_pos(path)?;
            Ok((path.clone(), *base, write_pos))
        }
        None => {
            let path = dir.join("0000000000000000.log");
            Ok((path, 0, 0))
        }
    }
}

/// Walk through the message headers to find the end of written data.
fn measure_write_pos(path: &Path) -> Result<usize, StreamError> {
    use crate::message::MessageHeader;

    let meta = std::fs::metadata(path).map_err(|e| StreamError::Io(e.to_string()))?;
    if meta.len() == 0 {
        return Ok(0);
    }

    let file = std::fs::File::open(path).map_err(|e| StreamError::Io(e.to_string()))?;
    let mmap = unsafe { memmap2::Mmap::map(&file) }
        .map_err(|e| StreamError::Io(e.to_string()))?;

    let mut pos = 0usize;
    let data = mmap.as_ref();

    while pos + MessageHeader::SIZE <= data.len() {
        let hdr: &MessageHeader = bytemuck::from_bytes(
            &data[pos..pos + MessageHeader::SIZE]
        );
        // Zero offset + zero length = unwritten region.
        if hdr.payload_len == 0 && hdr.offset == 0 && hdr.crc32 == 0 {
            break;
        }
        pos += MessageHeader::SIZE + hdr.payload_len as usize;
    }

    Ok(pos)
}

/// Open (or create) a segment file at `path` with `size` bytes pre-allocated,
/// returning a writable memory map.
fn open_mmap(path: &Path, size: usize) -> Result<MmapMut, StreamError> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(path)
        .map_err(|e| StreamError::Io(e.to_string()))?;

    // Extend the file to `size` if it is smaller.
    let current_len = file.metadata().map_err(|e| StreamError::Io(e.to_string()))?.len();
    if current_len < size as u64 {
        file.set_len(size as u64).map_err(|e| StreamError::Io(e.to_string()))?;
    }

    unsafe { MmapMut::map_mut(&file) }.map_err(|e| StreamError::Io(e.to_string()))
}
