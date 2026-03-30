//! Binary frame protocol for EustressStream network transport.
//!
//! Wire format: `[ 8-byte LE payload_len ][ bincode(Frame) ]`
//! Max frame size is enforced by `NodeConfig::frame_max_bytes`.

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::NodeError;

// ─────────────────────────────────────────────────────────────────────────────
// Frame types
// ─────────────────────────────────────────────────────────────────────────────

/// Frames sent from a client to a node.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ClientFrame {
    /// Subscribe to a topic from a given offset (None = next new message).
    Subscribe { topic: String, from_offset: Option<u64> },
    /// Cancel a topic subscription.
    Unsubscribe { topic: String },
    /// Publish a payload to a topic.
    Publish { topic: String, payload: Vec<u8> },
    /// Publish N messages across (potentially mixed) topics in one frame.
    /// Server returns `BatchAck { offsets: Vec<u64> }` — one offset per message.
    PublishBatch { messages: Vec<(String, Vec<u8>)> },
    /// **Zero-copy single-topic batch** — all payloads go to the same topic.
    ///
    /// Eliminates per-message topic string allocation and serialization.
    /// Server returns `BatchAckCompact { first_offset, count }` — just 12 bytes
    /// regardless of batch size (vs `N × 8` bytes for `BatchAck`).
    ///
    /// Prefer this over `PublishBatch` whenever all messages share one topic
    /// (e.g. `scene_deltas`, `agent_observations`). At batch-256 the ack shrinks
    /// from **2,048 → 12 bytes** — projected to push TCP throughput past 1M msg/s.
    PublishBatchTopic { topic: String, payloads: Vec<Vec<u8>> },

    /// **Fire-and-forget single publish** — server writes to the ring and sends
    /// **no response**. The client never blocks waiting for an ack.
    ///
    /// Throughput is limited only by TCP write bandwidth (~9M msg/s at 100B on
    /// loopback), not by round-trip latency. Safe for any best-effort stream
    /// (`scene_deltas`, `log/output`, `agent_observations`).
    PublishNoAck { topic: String, payload: Vec<u8> },

    /// **Fire-and-forget batch** — same as `PublishNoAck` but sends N messages
    /// in one frame. No ack of any kind is returned.
    ///
    /// This is the fastest TCP publish path. Use for high-frequency ECS delta
    /// emission where delivery of every individual message is not required.
    PublishBatchNoAck { topic: String, payloads: Vec<Vec<u8>> },

    /// List all active topics with stats.
    ListTopics,
    /// Health check — node replies with Pong.
    Ping,
}

/// Frames sent from a node to a client.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ServerFrame {
    /// A message delivered to a subscriber.
    Message { topic: String, offset: u64, timestamp: u64, payload: Vec<u8> },
    /// Response to ListTopics.
    TopicList(Vec<TopicStats>),
    /// Acknowledgement after a successful Publish; contains the assigned offset.
    Ack { offset: u64 },
    /// Acknowledgement for PublishBatch (mixed-topic); one offset per message.
    BatchAck { offsets: Vec<u64> },
    /// **Compact acknowledgement for PublishBatchTopic** (single-topic).
    ///
    /// Encodes the entire batch ack as `first_offset` + `count`:
    /// - `offsets[i] = first_offset + i`
    /// - Wire size: **12 bytes fixed** (vs `count × 8` bytes for `BatchAck`).
    BatchAckCompact { first_offset: u64, count: u32 },
    /// Error response.
    Error { code: u32, message: String },
    /// Health check reply.
    Pong,
}

/// Per-topic statistics reported in TopicList.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TopicStats {
    pub name: String,
    pub head: u64,
    pub subscriber_count: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Frame codec — write
// ─────────────────────────────────────────────────────────────────────────────

/// Encode and write a server frame to the writer.
pub async fn write_server_frame<W>(writer: &mut W, frame: &ServerFrame) -> Result<(), NodeError>
where
    W: AsyncWriteExt + Unpin,
{
    let payload = bincode::serialize(frame)
        .map_err(|e| NodeError::Encode(e.to_string()))?;
    write_framed(writer, &payload).await
}

/// Encode and write a client frame to the writer.
pub async fn write_client_frame<W>(writer: &mut W, frame: &ClientFrame) -> Result<(), NodeError>
where
    W: AsyncWriteExt + Unpin,
{
    let payload = bincode::serialize(frame)
        .map_err(|e| NodeError::Encode(e.to_string()))?;
    write_framed(writer, &payload).await
}

async fn write_framed<W>(writer: &mut W, payload: &[u8]) -> Result<(), NodeError>
where
    W: AsyncWriteExt + Unpin,
{
    let len = payload.len() as u64;
    writer.write_all(&len.to_le_bytes()).await?;
    writer.write_all(payload).await?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Frame codec — read
// ─────────────────────────────────────────────────────────────────────────────

/// Read a client frame from the reader.
pub async fn read_client_frame<R>(reader: &mut R, max_bytes: usize) -> Result<ClientFrame, NodeError>
where
    R: AsyncReadExt + Unpin,
{
    let payload = read_framed(reader, max_bytes).await?;
    bincode::deserialize(&payload)
        .map_err(|e| NodeError::Decode(e.to_string()))
}

/// Read a server frame from the reader.
pub async fn read_server_frame<R>(reader: &mut R, max_bytes: usize) -> Result<ServerFrame, NodeError>
where
    R: AsyncReadExt + Unpin,
{
    let payload = read_framed(reader, max_bytes).await?;
    bincode::deserialize(&payload)
        .map_err(|e| NodeError::Decode(e.to_string()))
}

async fn read_framed<R>(reader: &mut R, max_bytes: usize) -> Result<Bytes, NodeError>
where
    R: AsyncReadExt + Unpin,
{
    let mut len_buf = [0u8; 8];
    reader.read_exact(&mut len_buf).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            NodeError::ConnectionClosed
        } else {
            NodeError::Io(e)
        }
    })?;
    let len = u64::from_le_bytes(len_buf) as usize;

    if len > max_bytes {
        return Err(NodeError::FrameTooLarge(len));
    }

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            NodeError::ConnectionClosed
        } else {
            NodeError::Io(e)
        }
    })?;

    Ok(Bytes::from(buf))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn round_trip_publish() {
        let (mut a, mut b) = duplex(65536);
        let frame = ClientFrame::Publish {
            topic: "test".to_string(),
            payload: b"hello world".to_vec(),
        };
        write_client_frame(&mut a, &frame).await.unwrap();
        let decoded = read_client_frame(&mut b, 1024 * 1024).await.unwrap();
        assert!(matches!(decoded, ClientFrame::Publish { .. }));
    }

    #[tokio::test]
    async fn round_trip_message() {
        let (mut a, mut b) = duplex(65536);
        let frame = ServerFrame::Message {
            topic: "scene_deltas".to_string(),
            offset: 42,
            timestamp: 1234567890,
            payload: vec![1, 2, 3, 4],
        };
        write_server_frame(&mut a, &frame).await.unwrap();
        let decoded = read_server_frame(&mut b, 1024 * 1024).await.unwrap();
        assert!(matches!(decoded, ServerFrame::Message { offset: 42, .. }));
    }

    #[tokio::test]
    async fn frame_too_large() {
        let (mut a, mut b) = duplex(65536);
        let frame = ClientFrame::Publish {
            topic: "t".to_string(),
            payload: vec![0u8; 200],
        };
        write_client_frame(&mut a, &frame).await.unwrap();
        // max_bytes = 10 — should fail
        let result = read_client_frame(&mut b, 10).await;
        assert!(matches!(result, Err(NodeError::FrameTooLarge(_))));
    }
}
