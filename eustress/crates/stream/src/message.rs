//! Message types — owned `Message` and zero-copy `MessageView`.
//!
//! ## Zero-copy philosophy
//!
//! `MessageView<'a>` holds a `&'a [u8]` directly into the ring buffer slot.
//! No allocation occurs during delivery to subscribers.
//! Subscribers that need to own the data call `.to_owned()` to get `OwnedMessage`.

use bytes::Bytes;

// ─────────────────────────────────────────────────────────────────────────────
// MessageHeader — 32-byte fixed prefix written before every payload
// ─────────────────────────────────────────────────────────────────────────────

/// Fixed-size header prepended to every stored message.
/// `repr(C)` + bytemuck makes it safe for mmap/io_uring zero-copy reads.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MessageHeader {
    /// Monotonically increasing offset within the topic partition.
    pub offset:     u64,
    /// Unix timestamp (microseconds) when the message was produced.
    pub timestamp:  u64,
    /// CRC-32 of the payload bytes (0 = unchecked).
    pub crc32:      u32,
    /// Payload length in bytes (not including this header).
    pub payload_len: u32,
    /// Reserved — must be zero.
    pub _reserved:  [u8; 8],
}

// Safety: MessageHeader is a POD with no padding (C layout, all fields aligned).
unsafe impl bytemuck::Pod       for MessageHeader {}
unsafe impl bytemuck::Zeroable  for MessageHeader {}

impl MessageHeader {
    pub const SIZE: usize = std::mem::size_of::<MessageHeader>(); // 32 bytes

    pub fn new(offset: u64, payload: &[u8]) -> Self {
        let crc = crc32_fast(payload);
        let ts  = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        Self {
            offset,
            timestamp: ts,
            crc32: crc,
            payload_len: payload.len() as u32,
            _reserved: [0u8; 8],
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MessageView — zero-copy reference into a buffer slot
// ─────────────────────────────────────────────────────────────────────────────

/// Borrowed, zero-copy view of one message delivered to a subscriber callback.
///
/// Lifetime `'a` is tied to the ring buffer slot; the slot remains pinned
/// for the duration of the callback invocation.
#[derive(Clone, Copy)]
pub struct MessageView<'a> {
    pub topic:     &'a str,
    pub offset:    u64,
    pub timestamp: u64,
    pub data:      &'a [u8],
}

impl<'a> MessageView<'a> {
    /// Interpret the raw payload as a `T: bytemuck::Pod` without copying.
    #[inline]
    pub fn cast<T: bytemuck::Pod>(&self) -> Option<&T> {
        bytemuck::try_from_bytes(self.data).ok()
    }

    /// Clone the payload bytes into an owned value.
    #[inline]
    pub fn to_owned(self) -> OwnedMessage {
        OwnedMessage {
            topic:     self.topic.to_owned(),
            offset:    self.offset,
            timestamp: self.timestamp,
            data:      Bytes::copy_from_slice(self.data),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// OwnedMessage — heap-backed, Send + 'static
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OwnedMessage {
    pub topic:     String,
    pub offset:    u64,
    pub timestamp: u64,
    /// Refcounted bytes — cheap to clone after initial allocation.
    pub data:      Bytes,
}

impl OwnedMessage {
    /// Interpret the payload as `T: bytemuck::Pod` without copying.
    #[inline]
    pub fn cast<T: bytemuck::Pod>(&self) -> Option<&T> {
        bytemuck::try_from_bytes(&self.data).ok()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Fast CRC-32 (Castagnoli) suitable for data integrity checks.
/// Uses the standard CRC-32/ISO-HDLC polynomial.
fn crc32_fast(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}
