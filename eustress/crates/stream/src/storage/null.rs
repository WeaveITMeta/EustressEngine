//! NullBackend — no-op storage for in-memory-only streams.

use bytes::Bytes;
use std::sync::atomic::{AtomicU64, Ordering};

use super::StorageBackend;
use crate::error::StreamError;

pub struct NullBackend {
    pos: AtomicU64,
}

impl Default for NullBackend {
    fn default() -> Self { Self { pos: AtomicU64::new(0) } }
}

#[async_trait::async_trait]
impl StorageBackend for NullBackend {
    async fn append(&self, data: Bytes) -> Result<u64, StreamError> {
        let off = self.pos.fetch_add(data.len() as u64, Ordering::AcqRel);
        Ok(off)
    }

    async fn read_range(&self, _byte_offset: u64, _len: usize) -> Result<Bytes, StreamError> {
        Ok(Bytes::new())
    }

    async fn flush(&self) -> Result<(), StreamError> { Ok(()) }

    fn byte_len(&self) -> u64 { self.pos.load(Ordering::Acquire) }
}
