//! # The wire protocol, version 1
//!
//! What a host and a player say to each other, and how it is framed. Pure
//! data: no IO, no clocks, no threads, so the same file serves the desktop
//! transport and a browser one.
//!
//! ## Two lanes
//!
//! - **One reliable, ordered stream** per player, opened by the player. It
//!   carries [`ToHost`] and [`ToPlayer`] messages, each as one frame: a
//!   little-endian `u32` length, then a bincode body.
//! - **Unreliable datagrams** carry [`AvatarFrame`]s, where a late update is
//!   worth less than the next one. When a transport has no datagrams (a
//!   WebSocket fallback), the same frame rides the stream instead, as
//!   [`ToHost::Avatar`] / [`ToPlayer::Avatar`].
//!
//! ## Session, in order
//!
//! ```text
//! player                                   host
//!   Hello{protocol, name} ───────────────▶
//!                        ◀─────────────── Welcome{peer, world manifest, spawn}
//!   RequestChunks{hashes} ───────────────▶
//!                        ◀─────────────── ChunkPiece … (verified by blake3)
//!   (player opens the world, spawns its avatar)
//!   WorldReady ──────────────────────────▶
//!                        ◀─────────────── PeerJoined / Appearance for everyone present
//!   Appearance{descriptor} ──────────────▶ (relayed to everyone)
//!   AvatarFrame datagrams ◀═════════════▶ (host validates, stamps the peer id, relays)
//! ```
//!
//! Every decode is size-bounded ([`MAX_FRAME_BYTES`]), so a forged length
//! prefix cannot make a reader allocate more than one frame's worth.

use bincode::Options;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use eustress_echk::WorldManifest;

/// Bumped on any change a peer on the previous version could misread. A host
/// refuses a player whose version differs, with a reason a person can act on.
pub const PROTOCOL_VERSION: u16 = 1;

/// Largest reliable frame either side accepts.
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
/// World chunks cross the stream in pieces of this size, so one large chunk
/// never needs one large frame and never starves the avatar traffic behind it.
pub const CHUNK_PIECE_BYTES: usize = 256 * 1024;
/// Display names are cut to this many characters.
pub const MAX_NAME_CHARS: usize = 32;
/// Chat lines are cut to this many characters.
pub const MAX_CHAT_CHARS: usize = 400;
/// An avatar descriptor, as JSON, may not exceed this.
pub const MAX_APPEARANCE_BYTES: usize = 64 * 1024;

/// A participant in a session. The host is always [`HOST_PEER`].
pub type PeerId = u32;
/// The host's own peer id.
pub const HOST_PEER: PeerId = 0;

/// The player's first message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u16,
    pub name: String,
    /// Informational; shown in the host's log.
    pub engine_version: String,
}

/// The host's answer to an accepted [`Hello`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Welcome {
    pub protocol: u16,
    /// The id every other message about this player uses.
    pub peer: PeerId,
    pub host_name: String,
    /// The world to download before playing.
    pub world: WorldManifest,
    /// Where to place the player's avatar once the world is open.
    pub spawn: [f32; 3],
    pub max_players: u16,
}

/// Someone else in the session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer: PeerId,
    pub name: String,
}

/// Player → host, on the reliable stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToHost {
    Hello(Hello),
    /// Chunks the player does not already hold, by content address.
    RequestChunks { hashes: Vec<String> },
    /// The world is open and the avatar spawned; the player may now appear.
    WorldReady,
    /// The player's avatar, as `AvatarDescriptor` JSON.
    Appearance { descriptor_json: String },
    Chat { text: String },
    /// An avatar frame, when the transport has no datagrams.
    Avatar(AvatarFrame),
    Goodbye,
}

/// Host → player, on the reliable stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToPlayer {
    Welcome(Welcome),
    /// The join was refused; the connection closes after this.
    Refused { reason: String },
    /// One piece of a requested chunk. Pieces of a chunk arrive in order.
    ChunkPiece { hash: String, offset: u64, total: u64, bytes: Vec<u8> },
    /// A requested chunk the host does not have.
    ChunkMissing { hash: String },
    PeerJoined(PeerInfo),
    PeerLeft { peer: PeerId },
    Appearance { peer: PeerId, descriptor_json: String },
    Chat { peer: PeerId, name: String, text: String },
    /// An avatar frame, when the transport has no datagrams.
    Avatar(AvatarFrame),
    /// The host removed this player; the connection closes after this.
    Kicked { reason: String },
}

/// One sample of an avatar, sent unreliably about 20 times a second.
///
/// Carries the movement *intent* as well as the position, because a replica
/// is driven by the same avatar runtime as a local character: it walks with
/// the sender's intent, so it animates the way the sender's character does,
/// and the position pulls it back into step between samples.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AvatarFrame {
    /// Whose avatar. A player's own value is ignored; the host stamps it.
    pub peer: PeerId,
    /// Increases by one per frame, so a receiver can drop reordered ones.
    pub seq: u32,
    pub position: [f32; 3],
    /// Facing, radians about +Y.
    pub yaw: f32,
    /// World-space move direction, already normalised.
    pub direction: [f32; 3],
    pub vertical_velocity: f32,
    /// [`AvatarFrame::SPRINT`], [`AvatarFrame::CROUCH`], [`AvatarFrame::GROUNDED`].
    pub flags: u8,
    /// Jump presses so far, wrapping. A jump is an edge, and an edge sent
    /// unreliably can be lost; a counter cannot.
    pub jumps: u8,
}

impl AvatarFrame {
    pub const SPRINT: u8 = 1;
    pub const CROUCH: u8 = 2;
    pub const GROUNDED: u8 = 4;

    /// Every float is finite. A frame with a NaN is dropped, never applied.
    pub fn is_finite(&self) -> bool {
        self.position.iter().all(|v| v.is_finite())
            && self.direction.iter().all(|v| v.is_finite())
            && self.yaw.is_finite()
            && self.vertical_velocity.is_finite()
    }

    pub fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }
}

/// A frame or datagram that could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("frame of {0} bytes exceeds the frame size limit")]
    TooLarge(usize),
    #[error("malformed message: {0}")]
    Malformed(String),
}

fn options() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .with_limit(MAX_FRAME_BYTES as u64)
}

/// Encode a message as one stream frame: `u32` little-endian length, then
/// the bincode body.
pub fn encode_frame<T: Serialize>(msg: &T) -> Result<Vec<u8>, WireError> {
    let body = options().serialize(msg).map_err(|e| WireError::Malformed(e.to_string()))?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(WireError::TooLarge(body.len()));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Decode a frame body (the bytes after the length prefix).
pub fn decode_body<T: DeserializeOwned>(body: &[u8]) -> Result<T, WireError> {
    if body.len() > MAX_FRAME_BYTES {
        return Err(WireError::TooLarge(body.len()));
    }
    options().deserialize(body).map_err(|e| WireError::Malformed(e.to_string()))
}

/// Encode an avatar frame as one datagram (no length prefix; a datagram is
/// already delimited).
pub fn encode_datagram(frame: &AvatarFrame) -> Vec<u8> {
    // A fixed-size struct cannot exceed the limit.
    options().serialize(frame).unwrap_or_default()
}

/// Decode one datagram.
pub fn decode_datagram(bytes: &[u8]) -> Result<AvatarFrame, WireError> {
    decode_body(bytes)
}

/// Reassembles stream frames from arbitrarily sized reads.
///
/// The desktop transport reads whole frames with `read_exact`; a browser's
/// `ReadableStream` hands over chunks of any size, which is what this is for.
#[derive(Debug, Default)]
pub struct FrameAssembler {
    buf: Vec<u8>,
}

impl FrameAssembler {
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// The next complete frame body, if one has arrived. An error means the
    /// stream is corrupt and should be closed.
    pub fn next_body(&mut self) -> Result<Option<Vec<u8>>, WireError> {
        if self.buf.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
        if len > MAX_FRAME_BYTES {
            return Err(WireError::TooLarge(len));
        }
        if self.buf.len() < 4 + len {
            return Ok(None);
        }
        let body = self.buf[4..4 + len].to_vec();
        self.buf.drain(..4 + len);
        Ok(Some(body))
    }
}

/// A display name fit to show other players: control characters removed,
/// surrounding space trimmed, cut to [`MAX_NAME_CHARS`].
pub fn sanitize_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_NAME_CHARS)
        .collect()
}

/// A chat line fit to relay, or `None` when nothing printable remains.
pub fn sanitize_chat(raw: &str) -> Option<String> {
    let clean: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_CHAT_CHARS)
        .collect();
    let clean = clean.trim().to_string();
    (!clean.is_empty()).then_some(clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> AvatarFrame {
        AvatarFrame {
            peer: 3,
            seq: 42,
            position: [1.0, 2.0, 3.0],
            yaw: 0.5,
            direction: [0.0, 0.0, -1.0],
            vertical_velocity: -1.5,
            flags: AvatarFrame::SPRINT | AvatarFrame::GROUNDED,
            jumps: 255,
        }
    }

    #[test]
    fn frames_round_trip_through_the_assembler_in_any_split() {
        let msgs = vec![
            ToPlayer::Chat { peer: 1, name: "Ada".into(), text: "hi".into() },
            ToPlayer::ChunkPiece { hash: "ab".repeat(32), offset: 0, total: 3, bytes: vec![1, 2, 3] },
            ToPlayer::Avatar(frame()),
        ];
        let mut stream = Vec::new();
        for m in &msgs {
            stream.extend(encode_frame(m).unwrap());
        }
        // Feed one byte at a time: the worst case a browser stream can produce.
        let mut asm = FrameAssembler::default();
        let mut got: Vec<ToPlayer> = Vec::new();
        for b in &stream {
            asm.push(std::slice::from_ref(b));
            while let Some(body) = asm.next_body().unwrap() {
                got.push(decode_body(&body).unwrap());
            }
        }
        assert_eq!(got, msgs);
    }

    #[test]
    fn datagrams_round_trip_and_stay_small() {
        let d = encode_datagram(&frame());
        assert!(d.len() < 64, "avatar datagram grew to {} bytes", d.len());
        assert_eq!(decode_datagram(&d).unwrap(), frame());
    }

    #[test]
    fn forged_lengths_are_refused_before_allocation() {
        let mut asm = FrameAssembler::default();
        asm.push(&u32::MAX.to_le_bytes());
        assert!(matches!(asm.next_body(), Err(WireError::TooLarge(_))));

        // A body whose inner Vec length claims far more than the frame holds.
        let mut body = options().serialize(&ToHost::RequestChunks { hashes: vec![] }).unwrap();
        let n = body.len();
        body[n - 8..].copy_from_slice(&(u64::MAX / 2).to_le_bytes());
        assert!(decode_body::<ToHost>(&body).is_err());
    }

    #[test]
    fn names_and_chat_are_cleaned() {
        assert_eq!(sanitize_name("  Ada\u{7}  "), "Ada");
        assert_eq!(sanitize_name(&"x".repeat(100)).chars().count(), MAX_NAME_CHARS);
        assert_eq!(sanitize_chat("\n\t "), None);
        assert_eq!(sanitize_chat(" hello "), Some("hello".into()));
    }

    #[test]
    fn nan_frames_are_detected() {
        let mut f = frame();
        assert!(f.is_finite());
        f.position[1] = f32::NAN;
        assert!(!f.is_finite());
    }
}
