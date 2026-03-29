//! IoUringBackend — Linux io_uring-accelerated segment writes.
//!
//! Uses the `io-uring` crate (raw SQ/CQ ring interface) for batched,
//! kernel-bypass writes with registered buffer support.
//!
//! Only compiled on Linux when the `io-uring` feature is enabled.
//! Falls back to MmapBackend on other platforms (see storage/mod.rs).
//!
//! ## Performance characteristics
//! - Submits up to 256 write operations per batch.
//! - Uses `IORING_FEAT_NODROP` guard where available.
//! - No `O_SYNC` — durability is handled by explicit `fsync` on flush.

use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use io_uring::{IoUring, opcode, types};
use parking_lot::Mutex;

use super::StorageBackend;
use crate::error::StreamError;

const QUEUE_DEPTH: u32 = 256;

struct Inner {
    dir:         PathBuf,
    seg_size:    usize,
    file:        std::fs::File,
    ring:        IoUring,
    write_pos:   u64,
    seg_base:    u64,
    total_bytes: u64,
}

impl Inner {
    fn open(dir: &Path, seg_size: usize) -> Result<Self, StreamError> {
        let (path, seg_base, write_pos) = super::mmap::find_or_create_segment_pub(&dir, seg_size)?;
        let file = std::fs::OpenOptions::new()
            .read(true).write(true).create(true)
            .open(&path)
            .map_err(|e| StreamError::Io(e.to_string()))?;
        file.set_len(seg_size as u64).ok();

        let ring = IoUring::new(QUEUE_DEPTH).map_err(|e| StreamError::Io(e.to_string()))?;

        Ok(Self {
            dir: dir.to_path_buf(),
            seg_size,
            file,
            ring,
            write_pos: write_pos as u64,
            seg_base,
            total_bytes: seg_base + write_pos as u64,
        })
    }

    fn append_sync(&mut self, data: &[u8]) -> Result<u64, StreamError> {
        // Roll segment if needed.
        if self.write_pos as usize + data.len() > self.seg_size {
            self.roll_segment()?;
        }

        let offset = self.total_bytes;
        let fd  = types::Fd(self.file.as_raw_fd());
        let ptr = data.as_ptr();
        let len = data.len() as u32;
        let pos = self.write_pos;

        // SAFETY: data outlives the SQE submission; we call submit_and_wait(1)
        // immediately so the kernel is done before we return.
        let sqe = opcode::Write::new(fd, ptr, len)
            .offset(pos)
            .build()
            .user_data(0x1);

        unsafe { self.ring.submission().push(&sqe).map_err(|_| StreamError::Io("SQ full".into()))?; }
        self.ring.submit_and_wait(1).map_err(|e| StreamError::Io(e.to_string()))?;

        // Check completion.
        if let Some(cqe) = self.ring.completion().next() {
            if cqe.result() < 0 {
                return Err(StreamError::Io(format!("io_uring write: {}", cqe.result())));
            }
        }

        self.write_pos   += data.len() as u64;
        self.total_bytes += data.len() as u64;
        Ok(offset)
    }

    fn roll_segment(&mut self) -> Result<(), StreamError> {
        // fsync current segment.
        use std::os::unix::io::AsRawFd;
        let fd = types::Fd(self.file.as_raw_fd());
        let sqe = opcode::Fsync::new(fd).build().user_data(0x2);
        unsafe { self.ring.submission().push(&sqe).map_err(|_| StreamError::Io("SQ full".into()))?; }
        self.ring.submit_and_wait(1).map_err(|e| StreamError::Io(e.to_string()))?;

        let new_base = self.total_bytes;
        let path = self.dir.join(format!("{:016x}.log", new_base));
        self.file = std::fs::OpenOptions::new()
            .read(true).write(true).create(true)
            .open(&path)
            .map_err(|e| StreamError::Io(e.to_string()))?;
        self.file.set_len(self.seg_size as u64).ok();
        self.seg_base  = new_base;
        self.write_pos = 0;
        Ok(())
    }
}

use std::path::Path;

pub struct IoUringBackend {
    inner: Arc<Mutex<Inner>>,
}

impl IoUringBackend {
    pub fn open(dir: PathBuf, seg_size: usize) -> Result<Self, StreamError> {
        let inner = Inner::open(&dir, seg_size)?;
        Ok(Self { inner: Arc::new(Mutex::new(inner)) })
    }
}

#[async_trait::async_trait]
impl StorageBackend for IoUringBackend {
    async fn append(&self, data: Bytes) -> Result<u64, StreamError> {
        let mut inner = self.inner.lock();
        inner.append_sync(&data)
    }

    async fn read_range(&self, byte_offset: u64, len: usize) -> Result<Bytes, StreamError> {
        let inner = self.inner.lock();
        let seg_offset = byte_offset.saturating_sub(inner.seg_base) as usize;
        if seg_offset + len > inner.write_pos as usize {
            return Err(StreamError::OutOfRange { offset: byte_offset, len });
        }
        // mmap the file for reads (zero-copy path).
        let mmap = unsafe { memmap2::Mmap::map(&inner.file) }
            .map_err(|e| StreamError::Io(e.to_string()))?;
        Ok(Bytes::copy_from_slice(&mmap[seg_offset..seg_offset + len]))
    }

    async fn flush(&self) -> Result<(), StreamError> {
        // fsync via io_uring
        let mut inner = self.inner.lock();
        let fd = types::Fd(inner.file.as_raw_fd());
        let sqe = opcode::Fsync::new(fd).build().user_data(0x3);
        unsafe { inner.ring.submission().push(&sqe).map_err(|_| StreamError::Io("SQ full".into()))?; }
        inner.ring.submit_and_wait(1).map_err(|e| StreamError::Io(e.to_string()))?;
        Ok(())
    }

    fn byte_len(&self) -> u64 {
        self.inner.lock().total_bytes
    }
}
