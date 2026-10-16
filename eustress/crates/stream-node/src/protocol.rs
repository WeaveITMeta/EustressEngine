//! Binary frame protocol for EustressStream network transport.
//!
//! Wire format: `[ 8-byte LE payload_len ][ bincode(Frame) ]`
//! Max frame size is enforced by `NodeConfig::frame_max_bytes`.
//! Frames read before login are capped at 64 KB (`auth::PRE_LOGIN_MAX_FRAME`).
//!
//! bincode encodes an enum variant by its position, so new frames are only
//! ever appended to the end of each enum. Inserting one in the middle would
//! renumber every frame after it and break every older peer.
//!
//! Every frame that expects a reply gets exactly one, and replies come back
//! in the order the frames arrived, so a client matches them with one
//! first-in, first-out queue and needs no request ids. `Message` frames are
//! not replies.

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
    /// A logged-in party sees only the topics it holds a grant on.
    ListTopics,
    /// Health check — node replies with Pong.
    Ping,

    /// Login reply to `ServerFrame::Challenge`. Must be the first frame a
    /// client sends. `signature` is Ed25519 over the channel's login
    /// transcript (`auth::tcp_transcript`, `auth::quic_transcript`).
    Hello { protocol_version: u16, party_id: String, signature: Vec<u8> },
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
    /// A refused acknowledged publish or a refused subscribe (no grant, quota
    /// reached) is answered this way. A refused fire-and-forget publish gets
    /// no reply at all: its sender reads none.
    Error { code: u32, message: String },
    /// Health check reply.
    Pong,

    /// The first frame on every connection: the node id and a fresh 32-byte
    /// nonce for this connection's login transcript.
    Challenge { node_id: String, nonce: [u8; 32] },
    /// Login accepted. The connection now carries this party's grants.
    Welcome { party_id: String },
    /// Login refused, then the node closes the connection. `reason` is
    /// `"auth_failed"` or `"version_unsupported"`, nothing else.
    AuthFailed { reason: String },
    /// The success reply to `Subscribe`. `head` is the topic's head when the
    /// subscription starts. It is written before any `Message` for that
    /// subscription, replays included.
    Subscribed { head: u64 },
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
    // One write, then a flush. Over TLS two writes would be two records, and
    // a TLS stream holds written records until it is flushed, so a request
    // could wait on bytes still sitting in a buffer.
    let mut framed = Vec::with_capacity(8 + payload.len());
    framed.extend_from_slice(&len.to_le_bytes());
    framed.extend_from_slice(payload);
    writer.write_all(&framed).await?;
    writer.flush().await?;
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

/// Read one frame's payload bytes without decoding them.
pub async fn read_raw_frame<R>(reader: &mut R, max_bytes: usize) -> Result<Bytes, NodeError>
where
    R: AsyncReadExt + Unpin,
{
    read_framed(reader, max_bytes).await
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

    #[tokio::test]
    async fn round_trip_login_frames() {
        let (mut a, mut b) = duplex(65536);
        write_server_frame(&mut a, &ServerFrame::Challenge { node_id: "n".into(), nonce: [7; 32] }).await.unwrap();
        match read_server_frame(&mut b, 1024).await.unwrap() {
            ServerFrame::Challenge { node_id, nonce } => {
                assert_eq!(node_id, "n");
                assert_eq!(nonce, [7; 32]);
            }
            other => panic!("expected Challenge, got {other:?}"),
        }
        let hello = ClientFrame::Hello { protocol_version: 1, party_id: "p".into(), signature: vec![1; 64] };
        write_client_frame(&mut a, &hello).await.unwrap();
        assert!(matches!(read_client_frame(&mut b, 1024).await.unwrap(), ClientFrame::Hello { protocol_version: 1, .. }));
        write_server_frame(&mut a, &ServerFrame::Subscribed { head: 9 }).await.unwrap();
        assert!(matches!(read_server_frame(&mut b, 1024).await.unwrap(), ServerFrame::Subscribed { head: 9 }));
    }

    /// bincode numbers variants by position. The frames that existed before
    /// login keep their numbers, and the login frames come after them.
    #[test]
    fn variant_numbers_are_append_only() {
        fn index<T: Serialize>(frame: &T) -> u32 {
            let bytes = bincode::serialize(frame).unwrap();
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        }
        assert_eq!(index(&ClientFrame::Subscribe { topic: String::new(), from_offset: None }), 0);
        assert_eq!(index(&ClientFrame::ListTopics), 7);
        assert_eq!(index(&ClientFrame::Ping), 8);
        assert_eq!(index(&ClientFrame::Hello { protocol_version: 1, party_id: String::new(), signature: vec![] }), 9);
        assert_eq!(index(&ServerFrame::Message { topic: String::new(), offset: 0, timestamp: 0, payload: vec![] }), 0);
        assert_eq!(index(&ServerFrame::Error { code: 0, message: String::new() }), 5);
        assert_eq!(index(&ServerFrame::Pong), 6);
        assert_eq!(index(&ServerFrame::Challenge { node_id: String::new(), nonce: [0; 32] }), 7);
        assert_eq!(index(&ServerFrame::Welcome { party_id: String::new() }), 8);
        assert_eq!(index(&ServerFrame::AuthFailed { reason: String::new() }), 9);
        assert_eq!(index(&ServerFrame::Subscribed { head: 0 }), 10);
    }
}
