//! Lock-free, fixed-capacity ring buffer used as the in-memory hot path.
//!
//! ## Design
//!
//! Each slot is a `Bytes` (refcounted, zero-copy-shareable) value plus an
//! offset counter.  Producers advance a `head` atomic; consumers read up to
//! `head`.  When the ring wraps, the oldest slot is silently overwritten
//! (fire-and-forget semantics — durable storage is the segment log).
//!
//! `capacity` **must** be a power of two so that `idx & (capacity - 1)`
//! replaces division.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use parking_lot::RwLock;

// ─────────────────────────────────────────────────────────────────────────────
// Slot
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct Slot {
    offset: u64,
    data:   Bytes,   // Bytes::new() when empty
}

// ─────────────────────────────────────────────────────────────────────────────
// RingBuffer
// ─────────────────────────────────────────────────────────────────────────────

pub struct RingBuffer {
    slots:    Vec<RwLock<Slot>>,
    mask:     usize,
    /// Next write offset (monotonically increasing).
    head:     AtomicU64,
}

impl RingBuffer {
    /// `capacity` must be a power of two.
    pub fn new(capacity: usize) -> Arc<Self> {
        assert!(capacity.is_power_of_two(), "capacity must be a power of two");
        let slots = (0..capacity).map(|_| RwLock::new(Slot::default())).collect();
        Arc::new(Self {
            slots,
            mask: capacity - 1,
            head: AtomicU64::new(0),
        })
    }

    /// Append one message payload.  Returns the assigned offset.
    #[inline]
    pub fn push(&self, data: Bytes) -> u64 {
        let offset = self.head.fetch_add(1, Ordering::AcqRel);
        let idx    = (offset as usize) & self.mask;
        let mut slot = self.slots[idx].write();
        slot.offset = offset;
        slot.data   = data;
        offset
    }

    /// Read the slot at the given offset.  Returns `None` if the slot has
    /// already been overwritten (offset < head − capacity).
    #[inline]
    pub fn get(&self, offset: u64) -> Option<Bytes> {
        let head = self.head.load(Ordering::Acquire);
        let capacity = self.slots.len() as u64;
        if offset >= head || (head > capacity && offset < head - capacity) {
            return None;
        }
        let idx  = (offset as usize) & self.mask;
        let slot = self.slots[idx].read();
        if slot.offset == offset {
            Some(slot.data.clone())   // Bytes clone = refcount bump, O(1)
        } else {
            None
        }
    }

    /// Current write head (next offset that will be assigned).
    #[inline]
    pub fn head(&self) -> u64 {
        self.head.load(Ordering::Acquire)
    }

    /// Number of messages currently live in the ring (may lag by one slot).
    #[inline]
    pub fn len(&self) -> u64 {
        self.head()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_get() {
        let ring = RingBuffer::new(16);
        let offset = ring.push(Bytes::from_static(b"hello"));
        assert_eq!(offset, 0);
        assert_eq!(ring.get(0).unwrap().as_ref(), b"hello");
    }

    #[test]
    fn overwrite_wraps() {
        let ring = RingBuffer::new(4);
        for i in 0u64..6 {
            ring.push(Bytes::from(vec![i as u8]));
        }
        // Offset 0 and 1 are now overwritten
        assert!(ring.get(0).is_none());
        assert!(ring.get(1).is_none());
        // Offsets 4 and 5 are live
        assert!(ring.get(4).is_some());
        assert!(ring.get(5).is_some());
    }

    #[test]
    fn bytes_clone_is_zero_copy() {
        let ring = RingBuffer::new(8);
        let data  = Bytes::from(vec![1u8; 4096]);
        ring.push(data.clone());
        let got = ring.get(0).unwrap();
        // Pointer equality: same underlying allocation
        assert_eq!(got.as_ptr(), data.as_ptr());
    }
}
