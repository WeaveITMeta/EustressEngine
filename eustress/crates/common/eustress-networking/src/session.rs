//! # The session layer
//!
//! Bevy systems for hosting a session and for joining one. The Studio shell
//! hosts; the Player shell joins. Both add [`NetPlugin`] and differ only in
//! which session resource they insert.
//!
//! ## IO-free on purpose
//!
//! Nothing here opens a socket, spawns a thread, reads a clock other than
//! Bevy's [`Time`], or touches the filesystem. A transport sits on the far
//! side of [`NetLink`], a pair of channels carrying opaque frames, and every
//! system drains it with `try_recv`, never a blocking receive. The desktop
//! transport lives in [`crate::native`]; a browser build supplies its own and
//! reuses this file unchanged.
//!
//! ## What is replicated
//!
//! - **The world**, once, at join: the host's Space as `.echk` chunks, each
//!   verified against its content hash before it is used.
//! - **Avatars**, continuously. A replica is a real avatar spawned through
//!   [`SpawnAvatar`] with [`AvatarControl::Remote`], driven by the sender's
//!   movement intent and pulled toward the sender's position. It therefore
//!   walks, jumps and animates through the same runtime as the local
//!   character, not a second one.
//! - **Everything the host's scripts and physics do** ([`crate::repl`]): the
//!   host shell hands the session world frames ([`SendWorld`]) and motion
//!   ([`SendMotion`]); a player's session hands them to its shell
//!   ([`WorldArrived`], [`MotionArrived`]). A player that just arrived gets
//!   its catch-up before any live frame.
//! - **Each player's input** ([`crate::repl::input`]), every tick, kept on the
//!   host in [`PeerInputs`] for anything there to read.
//! - **Remote calls** from players ([`FireRemote`]), bounded and rate-limited
//!   here before the host shell sees them ([`RemoteArrived`]).
//!
//! Scripts run on the host only. A joined Player renders the world and the
//! avatars and sends its own input; it never runs a server script to see the
//! game.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use avian3d::collider_tree::ColliderTrees;
use avian3d::prelude::{AngularVelocity, ColliderOf, LinearVelocity, SpatialQuery, SpatialQueryFilter};
use bevy::math::Affine3A;
use bevy::prelude::*;
use crossbeam_channel::{unbounded, Receiver, Sender};
use serde::Serialize;

use eustress_common::avatar::rig::AvatarRig;
use eustress_common::avatar::seat::AvatarSeated;
use eustress_common::avatar::space_character::{SpaceCharacterPolicy, SpaceOwnsMotion};
use eustress_common::avatar::spawn::{AvatarBody, AvatarIntent, AvatarLocomotion};
use eustress_common::avatar::{
    resolve, AvatarControl, AvatarDescriptor, AvatarSystems, ItemId, LocalAvatar, MotionOverrides, ResolvedMotion,
    SpawnAvatar, SpawnedByAvatarRuntime, NOMINAL_BIND_HEIGHT_M,
};
use eustress_common::services::workspace::{live_gravity, Workspace};
use eustress_echk::{content_hash, decode_chunk, is_safe_record_path, Record, WorldManifest, MAX_CHUNK_BYTES};

use crate::repl::input::{
    encode_dir, key_bit, AXIS_LEFT_TRIGGER, AXIS_LEFT_X, AXIS_LEFT_Y, AXIS_RIGHT_TRIGGER, AXIS_RIGHT_X, AXIS_RIGHT_Y,
    INPUT_REDUNDANCY, MOUSE_LEFT, MOUSE_MIDDLE, MOUSE_RIGHT,
};
use crate::repl::motion::{tick_at, TICK_HZ};
use crate::repl::remote::check_call;
use crate::repl::tracks::MAX_TRACK_OPS_PER_MESSAGE;
use crate::repl::{
    InputFrame, InputSample, MotionFrame, PeerInputs, RemoteCall, RemoteRates, RemoteReply, TickClock, TrackWire,
    ValueLimits, WorldFrame,
};
use crate::wire::{
    decode_body, decode_datagram, encode_datagram, encode_frame, explain_transport_failure, sanitize_chat,
    sanitize_name, sanitize_receipts, sanitize_ticket, AvatarFrame, Datagram, Hello, PeerId, PeerInfo, ToHost,
    ToPlayer, Welcome, WireError, CHUNK_PIECE_BYTES, HOST_PEER, MAX_APPEARANCE_BYTES, MAX_SIM_ID_CHARS,
    PROTOCOL_VERSION,
};

/// Avatar samples per second, each direction.
pub const AVATAR_SEND_HZ: f32 = 20.0;
/// Avatar samples the host reads from one player per second, with a
/// second's worth banked for a burst after a hitch; more are dropped unread,
/// so a flood costs neither the host nor the players it would be relayed to.
const MAX_SAMPLES_PER_SECOND: f32 = 2.0 * AVATAR_SEND_HZ;
/// World bytes a host hands one player's stream per frame.
const CHUNK_BUDGET_PER_FRAME: usize = 1024 * 1024;
/// How much faster than its live top pace (walk, or capped sprint) an avatar
/// may cross the ground, for jitter and slopes.
const PACE_MARGIN: f32 = 1.15;
/// How much faster than its jump's take-off speed an avatar may rise.
const RISE_MARGIN: f32 = 1.5;
/// Speed every avatar may add on top, m/s, so a standing one can shuffle.
const PACE_SLACK_MPS: f32 = 0.5;
/// Seconds of travel an avatar may bank while it moves slower than its pace
/// (`TravelBudget`): enough for a stumble, never for a sprint across a map.
const BUDGET_SECONDS: f32 = 1.0;
/// The longest silence between accepted samples that still earns travel: a
/// stall (lost packets, a hitch on either side) is forgiven up to this, so a
/// peer that withholds samples can jump at most this many seconds of its pace,
/// once.
const STALL_CAP_S: f64 = 10.0;
/// Fastest an avatar may fall, m/s: past any real fall (700 m reaches
/// 117 m/s), short of dropping through a map in one sample.
const MAX_FALL_MPS: f32 = 150.0;
/// How fast a joined player may move before its replica exists, m/s: the
/// replica follows within frames, and a Space's own pace is not known yet.
const PRE_REPLICA_PACE_MPS: f32 = 30.0;
/// The longest silence that earns travel before the replica exists: it
/// spawns within frames, so a longer wait buys nothing.
const PRE_REPLICA_STALL_S: f64 = 2.0;
/// Seconds a replica keeps a pace a host script took away (a slow-down, a
/// freeze): the player hears of it a round trip later.
const MOTION_GRACE_S: f64 = 1.0;
/// Seconds between the corrections the host sends one player.
const CORRECTION_INTERVAL: f64 = 0.5;
/// How far below a replica's centre the host looks for what carries it, m.
const CARRIER_PROBE_M: f32 = 1.5;
/// Fastest a carrier may carry an avatar, m/s. A carrier that moves farther
/// in one frame than this pace allows (a host script snapping it somewhere)
/// moved its rider there once, and lent it no speed.
const CARRIER_CAP_MPS: f32 = 60.0;
/// Seconds an avatar keeps its carrier's speed after leaving it: a jump off a
/// fast platform carries the platform's speed through its arc.
const CARRIER_HOLD_S: f64 = 0.5;
/// Seconds between the changed appearances the host takes from one player; a
/// newer one waits until then.
const APPEARANCE_INTERVAL: f64 = 5.0;
/// Items a peer's descriptor may carry (the Worker's own cap).
const MAX_SLOTS: usize = 8;
/// Bone aliases a peer's rig may carry, and the longest name in one.
const MAX_BONE_ALIASES: usize = 128;
const MAX_BONE_NAME: usize = 64;
/// Farthest from the origin an avatar may be.
const WORLD_LIMIT: f32 = 1.0e5;
/// Position error past which a replica snaps rather than easing.
const SNAP_DISTANCE: f32 = 4.0;
/// Seconds a replica moved on this machine (a host script's teleport) keeps
/// its new place while its sender's samples still show the old one.
const TELEPORT_HOLD: f64 = 1.0;
/// Seconds without a sample before a replica stops walking.
const STALE_AFTER: f64 = 1.0;
/// Seconds a player waits for the host's Welcome.
const WELCOME_TIMEOUT: f64 = 20.0;
/// Link events handled per frame, so a flood cannot stall a frame.
const MAX_EVENTS_PER_FRAME: usize = 4096;
/// Rejected samples inside [`VIOLATION_WINDOW`] before the host removes a player.
const MAX_VIOLATIONS: u32 = 60;
const VIOLATION_WINDOW: f64 = 10.0;
/// Chat lines one player may send inside [`CHAT_WINDOW`].
const CHAT_BURST: usize = 8;
const CHAT_WINDOW: f64 = 10.0;
/// Receipt lists one player may send inside [`RECEIPT_WINDOW`]: each costs
/// the host API calls.
const RECEIPT_BURST: usize = 4;
const RECEIPT_WINDOW: f64 = 10.0;
/// Purchase prompts one player may have open at once.
const MAX_OPEN_PROMPTS: usize = 16;
/// Spacing of player spawn points around the host's spawn, so joiners do
/// not appear inside one another.
const SPAWN_RING_M: f32 = 1.6;

// ─────────────────────────────────────────────────────────────────────────────
// The link: the only boundary between a session and a transport
// ─────────────────────────────────────────────────────────────────────────────

/// A transport connection. A player's single connection to its host is
/// [`HOST_CONN`].
pub type ConnId = u64;
/// The connection id a player's link uses for its host.
pub const HOST_CONN: ConnId = 0;

/// Transport → session.
#[derive(Debug)]
pub enum LinkEvent {
    /// Host: the listener is up. `cert_sha256` is the pin players need.
    Listening { port: u16, cert_sha256: [u8; 32] },
    /// A session opened: on a host, a player arrived on `conn`; on a player,
    /// `conn` is [`HOST_CONN`] and the host answered.
    Opened { conn: ConnId, remote: String, datagrams: bool },
    /// One frame body from the reliable stream (length prefix removed).
    Frame { conn: ConnId, body: Vec<u8> },
    Datagram { conn: ConnId, bytes: Vec<u8> },
    Closed { conn: ConnId, reason: String },
    /// The transport itself failed: a bind, a connect, a certificate. Ends
    /// the session.
    Failed { reason: String },
}

/// Session → transport.
#[derive(Debug)]
pub enum LinkCommand {
    /// A complete frame, length prefix included (see [`encode_frame`]).
    Frame { conn: ConnId, frame: Vec<u8> },
    Datagram { conn: ConnId, bytes: Vec<u8> },
    Close { conn: ConnId, reason: String },
    /// Close everything and stop. Commands sent before this still go out.
    Shutdown,
}

/// The session's end of a transport. Present exactly while a session is.
/// Dropping it (removing the resource) shuts the transport down.
#[derive(Resource)]
pub struct NetLink {
    commands: Sender<LinkCommand>,
    events: Receiver<LinkEvent>,
}

/// The transport's end of a link.
pub struct LinkEnds {
    pub events: Sender<LinkEvent>,
    pub commands: Receiver<LinkCommand>,
}

/// A connected pair: the session keeps the [`NetLink`], the transport takes
/// the [`LinkEnds`].
pub fn link_pair() -> (NetLink, LinkEnds) {
    let (command_tx, command_rx) = unbounded();
    let (event_tx, event_rx) = unbounded();
    (
        NetLink { commands: command_tx, events: event_rx },
        LinkEnds { events: event_tx, commands: command_rx },
    )
}

impl NetLink {
    /// Frame and send a message on `conn`'s reliable stream.
    pub fn send<T: Serialize>(&self, conn: ConnId, msg: &T) {
        match encode_frame(msg) {
            Ok(frame) => {
                let _ = self.commands.send(LinkCommand::Frame { conn, frame });
            }
            Err(e) => warn!("net: dropped a message that would not encode: {e}"),
        }
    }

    pub fn datagram(&self, conn: ConnId, bytes: Vec<u8>) {
        let _ = self.commands.send(LinkCommand::Datagram { conn, bytes });
    }

    pub fn close(&self, conn: ConnId, reason: &str) {
        let _ = self.commands.send(LinkCommand::Close { conn, reason: reason.to_string() });
    }

    fn poll(&self) -> Option<LinkEvent> {
        self.events.try_recv().ok()
    }
}

impl Drop for NetLink {
    fn drop(&mut self) {
        let _ = self.commands.send(LinkCommand::Shutdown);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Messages a shell reads and writes
// ─────────────────────────────────────────────────────────────────────────────

/// A joining player's identity ticket in the host's hands: opaque, for the
/// API alone to read. Its `Debug` shows `Ticket(..)`, so no `{:?}` of a
/// notice or a peer prints it (`docs/networking/PLAYER_JOIN_TICKET.md`).
#[derive(Clone, PartialEq, Eq)]
pub struct Ticket(String);

impl Ticket {
    /// The ticket, for the API call that checks it.
    pub fn secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Ticket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ticket(..)")
    }
}

/// What happened, for a shell to show. Both roles emit these.
#[derive(Message, Debug, Clone)]
pub enum NetNotice {
    /// Host: listening. Compose the join link from these and the join key.
    Hosting { port: u16, pin: [u8; 32] },
    HostEnded { reason: String },
    /// Player: welcomed and in the world.
    Joined { peer: PeerId, host_name: String },
    JoinFailed { reason: String },
    Disconnected { reason: String },
    /// Player: world download progress, in bytes.
    Downloading { received: u64, total: u64 },
    PeerJoined { peer: PeerId, name: String },
    PeerLeft { peer: PeerId, name: String },
    Chat { peer: PeerId, name: String, text: String },
    /// Host: the player who just joined sent an identity ticket. Unverified:
    /// the shell checks it with the API before trusting the account it names.
    PeerIdentity { peer: PeerId, ticket: Ticket },
    /// Host: a player said `Hello` to a session the gallery lists, with the
    /// ticket it carried (none, possibly). The shell asks the gallery and
    /// answers with an [`AdmissionVerdict`]; the player waits until then, or
    /// [`ADMISSION_TIMEOUT`] at most.
    Admit { peer: PeerId, ticket: Option<Ticket> },
    /// Host: a player answered a purchase prompt this host sent it.
    PurchaseClosed { peer: PeerId, prompt: u32, purchased: bool },
    /// Host: purchase ids a player says its account holds. Unverified.
    Receipts { peer: PeerId, purchase_ids: Vec<String> },
    /// Player: the host asks this player to buy `product` (see
    /// [`crate::wire::ToPlayer::PurchasePrompt`]). Answer with [`ClosePurchase`].
    PurchasePrompt { prompt: u32, product: u64, expects: u8 },
}

/// A joined player's world, decoded and verified, ready for the shell to open.
pub struct DownloadedWorld {
    pub universe: String,
    /// The Space to open.
    pub start_space: String,
    /// Records of every Space, paths relative to that Space's folder.
    pub spaces: Vec<(String, Vec<Record>)>,
    /// The Universe's `assets/` records, paths relative to the Universe folder.
    pub assets: Vec<Record>,
    /// Where the host placed this player.
    pub spawn: [f32; 3],
}

/// The world arrived. The shell opens it, spawns the local avatar at
/// `spawn`, then writes [`LocalWorldReady`].
#[derive(Message)]
pub struct WorldDownloaded(pub Arc<DownloadedWorld>);

/// Player shell → session: the world is open and the avatar is spawned.
#[derive(Message, Debug, Clone, Default)]
pub struct LocalWorldReady;

/// Either shell → session: say something to everyone.
#[derive(Message, Debug, Clone)]
pub struct SendChat {
    pub text: String,
}

/// Either shell → session: end the session.
#[derive(Message, Debug, Clone)]
pub struct EndSession {
    pub reason: String,
}

/// Host shell → session: ask player `peer` to buy `product`. `prompt` is the
/// shell's own id for the ask; the answer arrives as
/// [`NetNotice::PurchaseClosed`] with the same id.
#[derive(Message, Debug, Clone)]
pub struct PromptPurchase {
    pub peer: PeerId,
    pub prompt: u32,
    pub product: u64,
    /// 0 for any product, 1 for a consumable, 2 for a pass.
    pub expects: u8,
}

/// Player shell → session: the player answered prompt `prompt`.
#[derive(Message, Debug, Clone)]
pub struct ClosePurchase {
    pub prompt: u32,
    pub purchased: bool,
}

/// Player shell → session: purchase ids the player's account holds for this
/// listing, for the host to verify and grant.
#[derive(Message, Debug, Clone)]
pub struct SendReceipts {
    pub purchase_ids: Vec<String>,
}

/// Host shell → session: world frames for one player, in order. `catch_up`
/// marks the frames that bring a player who just arrived up to date; until
/// those are sent, that player gets no live frames, which its catch-up
/// already includes.
#[derive(Message, Debug, Clone)]
pub struct SendWorld {
    pub peer: PeerId,
    pub frames: Vec<WorldFrame>,
    pub catch_up: bool,
}

/// Host shell → session: motion for every player in the world, each frame
/// already small enough for one datagram
/// ([`crate::repl::motion::split_motion`]).
#[derive(Message, Debug, Clone)]
pub struct SendMotion {
    pub frames: Vec<MotionFrame>,
}

/// Host shell → session: the answer to a player's `InvokeServer`.
#[derive(Message, Debug, Clone)]
pub struct SendRemoteReply {
    pub peer: PeerId,
    pub reply: RemoteReply,
}

/// Session → host shell: a player's remote call, its arguments bounded and
/// the player within its rate. The shell still checks that the player can
/// see the remote and every instance the arguments name.
#[derive(Message, Debug, Clone)]
pub struct RemoteArrived {
    pub peer: PeerId,
    pub call: RemoteCall,
    pub unreliable: bool,
}

/// Session → player shell: what the host's tree did in one tick.
#[derive(Message, Debug, Clone)]
pub struct WorldArrived(pub WorldFrame);

/// Session → player shell: where the host's moving bodies are.
#[derive(Message, Debug, Clone)]
pub struct MotionArrived(pub MotionFrame);

/// Session → player shell: the answer to an `InvokeServer`.
#[derive(Message, Debug, Clone)]
pub struct RemoteReplied(pub RemoteReply);

/// Player shell → session: `FireServer`, `InvokeServer`, or (with
/// `unreliable`) `UnreliableRemoteEvent:FireServer`.
#[derive(Message, Debug, Clone)]
pub struct FireRemote {
    pub call: RemoteCall,
    pub unreliable: bool,
}

/// Player shell → session: animation track changes this player's scripts
/// made on its own character, in order.
#[derive(Message, Debug, Clone)]
pub struct SendTracks(pub Vec<TrackWire>);

/// Session → host shell: a player's track changes, at most
/// [`MAX_TRACK_OPS_PER_MESSAGE`] at a time. The shell checks each one
/// (`repl::HostTracks::player_ops`).
#[derive(Message, Debug, Clone)]
pub struct TracksArrived {
    pub peer: PeerId,
    pub ops: Vec<TrackWire>,
}

/// Seconds a player waits for the gallery's admission before the host
/// refuses it.
pub const ADMISSION_TIMEOUT: f64 = 15.0;

/// What the host tells a player the gallery could not admit in time.
pub const ADMISSION_UNCONFIRMED: &str = "The gallery could not confirm this join right now. Join again from the gallery.";

/// The gallery admitted a player: its ticket's id, for the heartbeat, and
/// whether it plays as a guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admitted {
    pub ticket_id: String,
    pub guest: bool,
}

/// The shell's answer to a [`NetNotice::Admit`]: admitted, or refused with a
/// reason the player is shown.
#[derive(Message, Debug, Clone)]
pub struct AdmissionVerdict {
    pub peer: PeerId,
    pub outcome: Result<Admitted, String>,
}

/// Marks an avatar that mirrors another participant.
#[derive(Component, Debug, Clone, Copy)]
pub struct NetReplica {
    pub peer: PeerId,
}

// ─────────────────────────────────────────────────────────────────────────────
// Host session
// ─────────────────────────────────────────────────────────────────────────────

/// What a host serves.
pub struct HostConfig {
    /// Shown to players.
    pub host_name: String,
    /// The most players the session holds, everyone counted: the host's own
    /// character too while the host plays ([`HostSession::set_host_plays`]).
    /// The hosted Space's `Players.MaxPlayers`
    /// (`docs/networking/MAX_PLAYERS.md`).
    pub max_players: u16,
    /// The world players download.
    pub world: WorldManifest,
    /// Every chunk the manifest names, by content address.
    pub chunks: HashMap<String, Arc<Vec<u8>>>,
    /// Players spawn around this point.
    pub spawn: [f32; 3],
    /// The gallery listing the world is published as, when it is one.
    pub sim_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PeerStage {
    /// Connected; nothing accepted but `Hello`.
    AwaitingHello,
    /// Said `Hello` to a session the gallery lists; nothing of the world is
    /// sent until the gallery admits it ([`AdmissionVerdict`]).
    AwaitingAdmission,
    /// Welcomed; downloading the world.
    Downloading,
    /// In the world and visible to everyone.
    Ready,
}

struct HostPeer {
    peer: PeerId,
    name: String,
    stage: PeerStage,
    datagrams: bool,
    remote: String,
    appearance: Option<String>,
    /// Chunk pieces still to send: (hash, next offset).
    outbox: VecDeque<(String, usize)>,
    /// What its avatar may still travel, from its last accepted sample.
    travel: Option<TravelBudget>,
    /// When the host last sent it a [`ToPlayer::Correct`].
    corrected_at: f64,
    /// The avatar samples it may still send ([`MAX_SAMPLES_PER_SECOND`]).
    samples: SampleRate,
    /// When the host last took a changed appearance from it.
    appearance_at: f64,
    /// A changed appearance that came too soon after the last, kept until
    /// [`APPEARANCE_INTERVAL`] passes (the newest wins).
    pending_appearance: Option<(AvatarDescriptor, String)>,
    violations: u32,
    violation_window: f64,
    chat_times: VecDeque<f64>,
    /// The identity ticket its Hello carried, unverified, until the player
    /// enters the world: then it goes to the shell to check, and the peer
    /// keeps no copy.
    identity: Option<Ticket>,
    /// Purchase prompts sent to this player and not yet answered.
    prompts: HashSet<u32>,
    receipt_times: VecDeque<f64>,
    /// Its catch-up world frames went out; live ones may follow.
    caught_up: bool,
    /// Awaiting admission: when the host gives up and refuses it.
    admit_by: f64,
    /// The gallery's id for the ticket that admitted it, for the heartbeat.
    admitted: Option<String>,
    /// Admitted as a guest: its chat goes nowhere.
    guest: bool,
}

impl HostPeer {
    fn new(peer: PeerId, remote: String, datagrams: bool, now: f64) -> Self {
        Self {
            peer,
            name: String::new(),
            stage: PeerStage::AwaitingHello,
            datagrams,
            remote,
            appearance: None,
            outbox: VecDeque::new(),
            travel: None,
            corrected_at: f64::NEG_INFINITY,
            samples: SampleRate::new(now),
            appearance_at: f64::NEG_INFINITY,
            pending_appearance: None,
            violations: 0,
            violation_window: now,
            chat_times: VecDeque::new(),
            identity: None,
            prompts: HashSet::new(),
            receipt_times: VecDeque::new(),
            caught_up: false,
            admit_by: 0.0,
            admitted: None,
            guest: false,
        }
    }

    /// Count one thing the host refused from this player; true when that
    /// makes too many inside the window.
    fn violation(&mut self, now: f64) -> bool {
        if now - self.violation_window > VIOLATION_WINDOW {
            self.violation_window = now;
            self.violations = 0;
        }
        self.violations += 1;
        self.violations > MAX_VIOLATIONS
    }
}

/// The avatar samples one player may still send: [`MAX_SAMPLES_PER_SECOND`]
/// earned over time, a second's worth at most.
struct SampleRate {
    left: f32,
    at: f64,
}

impl SampleRate {
    fn new(now: f64) -> Self {
        Self { left: MAX_SAMPLES_PER_SECOND, at: now }
    }

    /// Whether the host reads one more sample now.
    fn take(&mut self, now: f64) -> bool {
        let earned = (now - self.at).max(0.0) as f32 * MAX_SAMPLES_PER_SECOND;
        self.left = (self.left + earned).min(MAX_SAMPLES_PER_SECOND);
        self.at = self.at.max(now);
        if self.left < 1.0 {
            return false;
        }
        self.left -= 1.0;
        true
    }
}

/// How fast a peer's avatar may go, m/s: across the ground, up and down; and
/// the longest silence that earns travel at that pace, s.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PaceLimit {
    across: f32,
    up: f32,
    down: f32,
    stall: f64,
}

impl PaceLimit {
    /// For an avatar whose own motion runs it at most `top` m/s and jumps it
    /// off the ground at `rise` m/s ([`own_paces`]), on a carrier moving at
    /// `carrier` m/s (a platform, a lift; at most [`CARRIER_CAP_MPS`]). It
    /// rises as fast as it runs or jumps, whichever is more, and falls at most
    /// [`MAX_FALL_MPS`]. A NaN or infinite speed counts as none.
    fn of(top: f32, rise: f32, carrier: f32) -> Self {
        let usable = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
        let across = usable(top) * PACE_MARGIN + PACE_SLACK_MPS;
        let up = (usable(rise) * RISE_MARGIN).max(across);
        let carrier = usable(carrier).min(CARRIER_CAP_MPS);
        Self { across: across + carrier, up: up + carrier, down: MAX_FALL_MPS + carrier, stall: STALL_CAP_S }
    }

    /// Before the replica exists ([`PRE_REPLICA_PACE_MPS`],
    /// [`PRE_REPLICA_STALL_S`]).
    fn before_replica() -> Self {
        Self { across: PRE_REPLICA_PACE_MPS, up: PRE_REPLICA_PACE_MPS, down: MAX_FALL_MPS, stall: PRE_REPLICA_STALL_S }
    }
}

/// The fastest an avatar moving by `motion` crosses the ground (its walk, or
/// its capped sprint) and takes off, m/s, under a downward gravity of
/// `gravity` m/s².
fn own_paces(motion: &ResolvedMotion, gravity: f32) -> (f32, f32) {
    (motion.walk_speed.max(motion.capped_run_and_sprint().1), motion.jump_velocity_under(gravity))
}

/// What the host knows of a peer's replica when one of its samples arrives.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Pace {
    /// How fast it may move.
    limit: PaceLimit,
    /// It rides a seat: the seat's vehicle moves it, at any speed.
    seated: bool,
    /// Where the replica is on the host (its capsule centre).
    place: Vec3,
    /// Its carrier jumped farther in one frame than any pace allows (a host
    /// script moved it): by how much, and when. Its rider may follow once.
    carried_jump: Option<(Vec3, f64)>,
}

/// Each joined player's [`Pace`], kept by the host from its replica every
/// frame ([`track_paces`]).
#[derive(Resource, Default)]
struct PeerPaces(HashMap<PeerId, PeerPace>);

impl PeerPaces {
    fn pace(&self, peer: PeerId) -> Option<Pace> {
        self.0.get(&peer).map(|p| p.pace)
    }

    /// The rider followed its carrier's jump: it may not again.
    fn followed_carrier(&mut self, peer: PeerId) {
        if let Some(p) = self.0.get_mut(&peer) {
            p.pace.carried_jump = None;
            p.carried_jump = None;
        }
    }
}

/// What the host keeps of one replica between frames.
struct PeerPace {
    pace: Pace,
    recent: RecentPaces,
    /// The replica's centre last frame.
    last_place: Option<Vec3>,
    /// What it stood on last frame: the body, the point under it in that
    /// body's own frame, and the body's transform then.
    under: Option<(Entity, Vec3, Affine3A)>,
    /// The carrier it rides now, and the speed it lends, m/s.
    riding: Option<(Entity, f32)>,
    /// The speed of a carrier it left, m/s, lent until the time given.
    left: Option<(f32, f64)>,
    carried_jump: Option<(Vec3, f64)>,
}

/// A carrier under a replica this frame, as the host sees it.
struct CarrierSighting {
    body: Entity,
    /// Where the replica stands on it, world space.
    point: Vec3,
    /// The body's transform now.
    transform: Affine3A,
    /// Its physics velocity at `point`, m/s (zero for a body without one,
    /// such as a part a script tweens).
    velocity: Vec3,
}

/// Faster than this downward, m/s, relative to what is under it, a replica
/// is falling past it, not riding it.
const FALL_AWAY_MPS: f32 = 4.0;

impl PeerPace {
    fn new(place: Vec3) -> Self {
        Self {
            pace: Pace { limit: PaceLimit::before_replica(), seated: false, place, carried_jump: None },
            recent: RecentPaces::default(),
            last_place: None,
            under: None,
            riding: None,
            left: None,
            carried_jump: None,
        }
    }

    /// The speed what carries the replica lends it this frame, m/s: the
    /// faster of how the point it stands on moved since last frame (a part
    /// a script tweens) and that point's physics velocity, at most
    /// [`CARRIER_CAP_MPS`]. A carrier it left (or changed from) lends its
    /// last speed [`CARRIER_HOLD_S`] longer, through a jump's arc. A carrier
    /// that jumped farther than that pace allows lends no speed; its jump is
    /// kept for the rider to follow once (`carried_jump`).
    fn carried_speed(&mut self, sighting: Option<CarrierSighting>, place: Vec3, dt: f32, now: f64) -> f32 {
        let rider = match self.last_place {
            Some(last) if dt > 0.0 => (place - last) / dt,
            _ => Vec3::ZERO,
        };
        self.last_place = Some(place);
        let mut riding = None;
        let mut under = None;
        if let Some(s) = sighting {
            let local = s.transform.inverse().transform_point3(s.point);
            if local.is_finite() {
                under = Some((s.body, local, s.transform));
                let moved = match self.under {
                    Some((body, then_local, then)) if body == s.body => {
                        s.transform.transform_point3(then_local) - then.transform_point3(then_local)
                    }
                    _ => Vec3::ZERO,
                };
                if moved.length() > SNAP_DISTANCE + CARRIER_CAP_MPS * dt {
                    self.carried_jump = Some((moved, now));
                    riding = Some((s.body, 0.0));
                } else {
                    let tweened = if dt > 0.0 { moved / dt } else { Vec3::ZERO };
                    let v = if tweened.length() >= s.velocity.length() { tweened } else { s.velocity };
                    // Falling past it is not riding it.
                    if v.is_finite() && rider.y - v.y >= -FALL_AWAY_MPS {
                        riding = Some((s.body, v.length().min(CARRIER_CAP_MPS)));
                    }
                }
            }
        }
        self.under = under;
        if let Some((body, speed)) = self.riding {
            let left_it = riding.map_or(true, |(on, _)| on != body);
            if left_it && self.left.map_or(true, |(held, until)| now > until || speed >= held) {
                self.left = Some((speed, now + CARRIER_HOLD_S));
            }
        }
        self.riding = riding;
        let left = self.left.filter(|&(_, until)| now <= until).map_or(0.0, |(speed, _)| speed);
        riding.map_or(0.0, |(_, speed)| speed).max(left)
    }
}

/// Changes of an avatar's own paces (`top`, `rise` as [`own_paces`] gives
/// them) kept the last [`MOTION_GRACE_S`]: each from when it began. A pace
/// a host script takes away (a slow-down, a WalkSpeed 0 freeze) counts for
/// that long after, while the player hears of it.
#[derive(Default)]
struct RecentPaces(VecDeque<(f64, f32, f32)>);

/// Pace changes [`RecentPaces`] keeps; past this, the oldest two merge into
/// the faster of each, which only ever allows more.
const RECENT_PACES: usize = 8;

impl RecentPaces {
    /// Note the avatar's paces now; returns the fastest of the last
    /// [`MOTION_GRACE_S`].
    fn note(&mut self, now: f64, top: f32, rise: f32) -> (f32, f32) {
        if self.0.back().map_or(true, |&(_, t, r)| t != top || r != rise) {
            if self.0.len() >= RECENT_PACES {
                if let Some((_, t, r)) = self.0.pop_front() {
                    if let Some(next) = self.0.front_mut() {
                        next.1 = next.1.max(t);
                        next.2 = next.2.max(r);
                    }
                }
            }
            self.0.push_back((now, top, rise));
        }
        // A pace stops counting MOTION_GRACE_S after the next one began.
        while self.0.len() > 1 && now - self.0[1].0 >= MOTION_GRACE_S {
            self.0.pop_front();
        }
        self.0.iter().fold((0.0f32, 0.0f32), |(t, r), &(_, top, rise)| (t.max(top), r.max(rise)))
    }
}

/// Whether `entity` is an avatar or part of one (a held tool, say): no
/// avatar carries another, or players standing on each other would add up
/// their speeds.
fn is_avatar_part(entity: Entity, is_avatar: impl Fn(Entity) -> bool, parent: impl Fn(Entity) -> Option<Entity>) -> bool {
    let mut at = Some(entity);
    // Deep enough for a tool in a hand in a rig; a deeper chain is a part of
    // the world.
    for _ in 0..16 {
        let Some(e) = at else { return false };
        if is_avatar(e) {
            return true;
        }
        at = parent(e);
    }
    false
}

/// Keep each joined player's [`Pace`] from its replica: the fastest its own
/// motion allowed in the last [`MOTION_GRACE_S`], whether it is seated, and
/// the speed whatever carries it lends it.
///
/// Its own motion is the replica's live `AvatarBody.motion`, which host
/// scripts change (`Humanoid.WalkSpeed`), or the motion its descriptor gives
/// it, whichever is faster. That floor is TEMPORARY: a Player's avatar loses
/// a script's WalkSpeed when its rig binds or it respawns, so an honest
/// player can move at its descriptor's pace after a script slowed it. Once
/// that is fixed the floor goes, and a freeze (WalkSpeed 0) holds.
#[allow(clippy::too_many_arguments)]
fn track_paces(
    time: Res<Time>,
    remotes: Res<RemoteAvatars>,
    mut paces: ResMut<PeerPaces>,
    workspace: Option<Res<Workspace>>,
    spatial: SpatialQuery,
    replicas: Query<(&AvatarBody, &AvatarDescriptor, Option<&AvatarRig>, Has<AvatarSeated>, &GlobalTransform)>,
    colliders: Query<(&GlobalTransform, Option<&ColliderOf>)>,
    velocities: Query<(&GlobalTransform, &LinearVelocity, Option<&AngularVelocity>)>,
    avatars: Query<(), With<AvatarBody>>,
    parents: Query<&ChildOf>,
) {
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs();
    let gravity = -live_gravity(workspace.as_deref()).y;
    let mut seen = HashSet::new();
    for (peer, r) in &remotes.peers {
        let Some((body, descriptor, rig, seated, at)) = r.entity.and_then(|e| replicas.get(e).ok()) else { continue };
        seen.insert(*peer);
        let place = at.translation();
        let track = paces.0.entry(*peer).or_insert_with(|| PeerPace::new(place));

        let (top, rise) = own_paces(&body.motion, gravity);
        let (top, rise) = track.recent.note(now, top, rise);
        let bind_height = rig.map_or(NOMINAL_BIND_HEIGHT_M, |rig| rig.bind_height_m);
        let (floor_top, floor_rise) = own_paces(&resolve(descriptor, bind_height).1, gravity);

        let hit = spatial.cast_ray_predicate(
            place,
            Dir3::NEG_Y,
            CARRIER_PROBE_M,
            true,
            &SpatialQueryFilter::default(),
            &|e| !is_avatar_part(e, |a| avatars.contains(a), |c| parents.get(c).ok().map(|p| p.parent())),
        );
        let sighting = hit.and_then(|hit| {
            let point = place + Vec3::NEG_Y * hit.distance;
            let (transform, of) = colliders.get(hit.entity).ok()?;
            let velocity = velocities
                .get(of.map_or(hit.entity, |of| of.body))
                .map(|(centre, linear, angular)| {
                    linear.0 + angular.map_or(Vec3::ZERO, |w| w.0.cross(point - centre.translation()))
                })
                .unwrap_or(Vec3::ZERO);
            Some(CarrierSighting { body: hit.entity, point, transform: transform.affine(), velocity })
        });
        let carrier = track.carried_speed(sighting, place, dt, now);

        track.pace = Pace {
            limit: PaceLimit::of(top.max(floor_top), rise.max(floor_rise), carrier),
            seated,
            place,
            carried_jump: track.carried_jump.filter(|&(_, since)| now - since < TELEPORT_HOLD),
        };
    }
    paces.0.retain(|peer, _| seen.contains(peer));
}

/// What a peer's avatar may still travel since its last accepted sample:
/// metres across the ground, up and down. Each sample earns travel at the
/// avatar's pace for the time since the last accepted one (at most the
/// pace's `stall`, so a stall is forgiven), spends what it moved, and keeps
/// at most [`BUDGET_SECONDS`] of pace in the bank, so standing still never
/// buys a sprint and a sustained run above the pace is refused.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TravelBudget {
    at: f64,
    pos: Vec3,
    across: f32,
    up: f32,
    down: f32,
}

impl TravelBudget {
    /// A full bank at `pos`.
    fn start(now: f64, pos: Vec3, limit: PaceLimit) -> Self {
        Self {
            at: now,
            pos,
            across: limit.across * BUDGET_SECONDS,
            up: limit.up * BUDGET_SECONDS,
            down: limit.down * BUDGET_SECONDS,
        }
    }

    /// Spend the travel from the last accepted sample to `pos`. False when it
    /// overspends: the sample is refused and the budget is left as it was, so
    /// the time since still counts for the next one.
    fn spend(&mut self, now: f64, pos: Vec3, limit: PaceLimit) -> bool {
        let earned = (now - self.at).clamp(0.0, limit.stall) as f32;
        let moved = pos - self.pos;
        let crossed = Vec2::new(moved.x, moved.z).length();
        let (rose, fell) = (moved.y.max(0.0), (-moved.y).max(0.0));
        let across = self.across + limit.across * earned;
        let up = self.up + limit.up * earned;
        let down = self.down + limit.down * earned;
        if crossed > across || rose > up || fell > down {
            return false;
        }
        *self = Self {
            at: now,
            pos,
            across: (across - crossed).min(limit.across * BUDGET_SECONDS),
            up: (up - rose).min(limit.up * BUDGET_SECONDS),
            down: (down - fell).min(limit.down * BUDGET_SECONDS),
        };
        true
    }
}

/// A running host. Inserted by the Studio shell together with a [`NetLink`].
#[derive(Resource)]
pub struct HostSession {
    config: HostConfig,
    port: u16,
    pin: Option<[u8; 32]>,
    peers: HashMap<ConnId, HostPeer>,
    next_peer: PeerId,
    appearance: Option<String>,
    /// The host's own identity ticket, bound to its pin; sent in every Welcome.
    identity: Option<String>,
    rates: RemoteRates,
    /// Joins wait for the gallery's admission ([`Self::require_admission`]).
    admission: bool,
    /// The host's own character is in the session, and counts as a player.
    host_plays: bool,
}

impl HostSession {
    pub fn new(config: HostConfig) -> Self {
        Self {
            config,
            port: 0,
            pin: None,
            peers: HashMap::new(),
            next_peer: 1,
            appearance: None,
            identity: None,
            rates: RemoteRates::default(),
            admission: false,
            host_plays: false,
        }
    }

    /// Whether the host's own character is in the session: the shell says so
    /// each frame. It counts as one of [`HostConfig::max_players`]; a host
    /// with no character (headless, or hosting from Edit) counts as nobody.
    pub fn set_host_plays(&mut self, plays: bool) {
        self.host_plays = plays;
    }

    /// Whether the host's own character counts in the session.
    pub fn host_plays(&self) -> bool {
        self.host_plays
    }

    /// Whether one more player fits: everyone counted, those still joining
    /// included, so two arriving together never overfill it.
    fn has_room(&self) -> bool {
        self.peers.len() + usize::from(self.host_plays) < usize::from(self.config.max_players)
    }

    /// Why a joining player is turned away when the session is full: its
    /// places, those still joining included, of what the Space holds.
    fn full_reason(&self) -> String {
        format!(
            "This session is full ({} of {} players).",
            self.peers.len() + usize::from(self.host_plays),
            self.config.max_players
        )
    }

    /// Players in the world, and whether each has had its catch-up.
    pub fn ready_peers(&self) -> Vec<(PeerId, bool)> {
        self.ready().map(|(_, p)| (p.peer, p.caught_up)).collect()
    }

    /// Set the host's own identity ticket (audience: its pin, as 64 hex
    /// characters), for players to verify who hosts. The pin exists only once
    /// [`NetNotice::Hosting`] arrives, so the shell fetches the ticket then.
    /// Players who joined before it is set received none.
    pub fn set_identity(&mut self, ticket: Option<String>) {
        self.identity = ticket.as_deref().and_then(sanitize_ticket);
    }

    /// The gallery listing this session's world is published as, if any.
    pub fn sim_id(&self) -> Option<&str> {
        self.config.sim_id.as_deref()
    }

    /// The bound port, once the listener is up.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The certificate pin, once the listener is up.
    pub fn pin(&self) -> Option<[u8; 32]> {
        self.pin
    }

    /// From now on every player who says `Hello` waits for the gallery to
    /// admit it (`docs/networking/HOSTED_ADMISSION.md`). The shell calls this
    /// as it sends a heartbeat that lists the session's link, so no join gets
    /// in while the gallery stores the link; once the gallery has listed it,
    /// it holds for the rest of the session, since the link is out by then.
    pub fn require_admission(&mut self) {
        if !self.admission {
            info!("net: the gallery is listing this session; joins now need its admission");
        }
        self.admission = true;
    }

    /// Joins stop waiting for the gallery. Only for a session the gallery
    /// never listed, whose heartbeat it certainly stored nothing of (it
    /// refused it, or the request never left): the link is not out. A player
    /// already waiting is answered by its own admission request.
    pub fn release_admission(&mut self) {
        if self.admission {
            info!("net: the gallery did not list this session; joins no longer wait for it");
        }
        self.admission = false;
    }

    /// Whether joins need the gallery's admission.
    pub fn requires_admission(&self) -> bool {
        self.admission
    }

    /// The gallery's ticket ids of the players still connected, for the
    /// heartbeat that keeps their guest leases running.
    pub fn admitted_ids(&self) -> Vec<String> {
        self.peers
            .values()
            .filter(|p| matches!(p.stage, PeerStage::Downloading | PeerStage::Ready))
            .filter_map(|p| p.admitted.clone())
            .collect()
    }

    /// Players in the world, everyone counted: the host's own character
    /// too while the host plays.
    pub fn player_count(&self) -> usize {
        self.peers.values().filter(|p| p.stage == PeerStage::Ready).count() + usize::from(self.host_plays)
    }

    /// The most players the session holds, everyone counted
    /// ([`HostConfig::max_players`]).
    pub fn max_players(&self) -> u16 {
        self.config.max_players
    }

    /// Names of players in the world.
    pub fn player_names(&self) -> Vec<String> {
        self.peers.values().filter(|p| p.stage == PeerStage::Ready).map(|p| p.name.clone()).collect()
    }

    fn ready(&self) -> impl Iterator<Item = (ConnId, &HostPeer)> {
        self.peers.iter().filter(|(_, p)| p.stage == PeerStage::Ready).map(|(c, p)| (*c, p))
    }

    fn spawn_for(&self, peer: PeerId) -> [f32; 3] {
        let a = peer as f32 * 2.399_963; // golden angle: successive players spread evenly
        let [x, y, z] = self.config.spawn;
        [x + a.cos() * SPAWN_RING_M, y, z + a.sin() * SPAWN_RING_M]
    }

    fn unique_name(&self, wanted: &str, peer: PeerId) -> String {
        let base = if wanted.is_empty() { format!("Player{peer}") } else { wanted.to_string() };
        let taken = |n: &str| n == self.config.host_name || self.peers.values().any(|p| p.name == n);
        if !taken(&base) {
            return base;
        }
        (2..).map(|i| format!("{base} ({i})")).find(|n| !taken(n)).unwrap_or(base)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Player session
// ─────────────────────────────────────────────────────────────────────────────

/// Where a joining player is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinStage {
    /// The transport is connecting.
    Connecting,
    /// `Hello` sent; waiting for the host.
    AwaitingWelcome,
    /// Downloading the world.
    Downloading,
    /// The world arrived; the shell is opening it.
    Opening,
    /// In the world.
    Playing,
}

/// The app this machine runs, with its version (`eustress-client 0.1.0`),
/// which a joining player tells the host in its [`Hello`]. The Player shell
/// inserts it; without it, the Hello names this crate's version.
#[derive(Resource, Debug, Clone)]
pub struct AppVersion(pub String);

/// A joining or joined player. Inserted by the Player shell together with a
/// [`NetLink`].
#[derive(Resource)]
pub struct PlayerSession {
    name: String,
    /// This player's identity ticket, until [`PlayerSession::hello`] takes it
    /// into the one Hello.
    identity: Option<String>,
    /// From Welcome: the listing, and the host's own ticket.
    sim_id: Option<String>,
    host_identity: Option<String>,
    stage: JoinStage,
    stage_since: Option<f64>,
    peer: Option<PeerId>,
    host_name: String,
    datagrams: bool,
    world: Option<WorldManifest>,
    spawn: [f32; 3],
    sizes: HashMap<String, u64>,
    wanted: BTreeSet<String>,
    partial: HashMap<String, Vec<u8>>,
    have: HashMap<String, Arc<Vec<u8>>>,
    received: u64,
    total: u64,
    last_progress: f64,
    names: HashMap<PeerId, String>,
    /// The host's clock, as the ticks its frames carry reveal it.
    clock: TickClock,
    /// The newest input samples sent, newest first.
    input_sent: VecDeque<InputSample>,
    input_accum: f32,
}

impl PlayerSession {
    /// The Hello this player opens with. It takes the identity ticket: the
    /// ticket goes into this one Hello, and the session keeps no copy
    /// (`docs/networking/PLAYER_JOIN_TICKET.md`).
    fn hello(&mut self, engine_version: String) -> Hello {
        Hello { protocol: PROTOCOL_VERSION, name: self.name.clone(), engine_version, identity: self.identity.take() }
    }

    pub fn new(name: &str, identity: Option<String>) -> Self {
        Self {
            name: sanitize_name(name),
            identity: identity.as_deref().and_then(sanitize_ticket),
            sim_id: None,
            host_identity: None,
            stage: JoinStage::Connecting,
            stage_since: None,
            peer: None,
            host_name: String::new(),
            datagrams: false,
            world: None,
            spawn: [0.0; 3],
            sizes: HashMap::new(),
            wanted: BTreeSet::new(),
            partial: HashMap::new(),
            have: HashMap::new(),
            received: 0,
            total: 0,
            last_progress: 0.0,
            names: HashMap::new(),
            clock: TickClock::default(),
            input_sent: VecDeque::new(),
            input_accum: 0.0,
        }
    }

    pub fn stage(&self) -> JoinStage {
        self.stage
    }

    /// The host's tick now (fractional), once any frame has revealed it.
    /// `local_secs` is this app's `Time::elapsed_secs_f64`.
    pub fn host_tick(&self, local_secs: f64) -> Option<f64> {
        self.clock.host_tick(local_secs)
    }

    /// The host's clock, for drawing its bodies a little in the past.
    pub fn clock(&self) -> &TickClock {
        &self.clock
    }

    pub fn peer(&self) -> Option<PeerId> {
        self.peer
    }

    pub fn host_name(&self) -> &str {
        &self.host_name
    }

    /// The gallery listing the host's world is published as, if it said so.
    pub fn sim_id(&self) -> Option<&str> {
        self.sim_id.as_deref()
    }

    /// The host's own identity ticket, unverified: check it with the API
    /// (audience: the join link's pin) before trusting who hosts.
    pub fn host_identity(&self) -> Option<&str> {
        self.host_identity.as_deref()
    }

    fn set_stage(&mut self, stage: JoinStage) {
        self.stage = stage;
        self.stage_since = None; // stamped on the next frame from Time
    }
}

/// Where a joining player keeps chunks between sessions, so rejoining an
/// unchanged world downloads nothing. Optional; without one every join
/// downloads the whole world. Native shells back it with a directory, a
/// browser with memory or IndexedDB.
pub trait ChunkCache: Send + Sync + 'static {
    fn get(&self, hash: &str) -> Option<Vec<u8>>;
    fn put(&self, hash: &str, bytes: &[u8]);
}

/// The shell's chunk cache, if it provides one.
#[derive(Resource)]
pub struct ChunkCacheRes(pub Box<dyn ChunkCache>);

// ─────────────────────────────────────────────────────────────────────────────
// Avatars
// ─────────────────────────────────────────────────────────────────────────────

struct Remote {
    name: String,
    appearance: Option<AvatarDescriptor>,
    latest: Option<AvatarFrame>,
    received_at: f64,
    applied_jumps: Option<u8>,
    entity: Option<Entity>,
    /// Its look changed: a new replica replaces `entity` once it is built.
    respawn: bool,
    /// The look `entity` was built with, while a changed one is rebuilt: if
    /// the new body never appears, the old one stays, in this look.
    previous_look: Option<AvatarDescriptor>,
    /// Where the correction left the replica last frame.
    placed: Option<Vec3>,
    /// Something on this machine moved the replica (a host script's
    /// teleport): where to, and when.
    teleport_hold: Option<(Vec3, f64)>,
}

impl Remote {
    /// Where to pull this peer's replica now, or `None` to leave it. A move
    /// on this machine farther than a snap since the last correction (a host
    /// script teleporting a joined player) is kept: the sender's samples,
    /// from before it heard of the move, are ignored until one lands near
    /// the new place, or for at most [`TELEPORT_HOLD`] seconds.
    fn correction_target(&mut self, current: Vec3, sample: Vec3, now: f64) -> Option<Vec3> {
        if self.teleport_hold.is_none() && self.placed.is_some_and(|p| current.distance(p) > SNAP_DISTANCE) {
            self.teleport_hold = Some((current, now));
        }
        if let Some((to, since)) = self.teleport_hold {
            if sample.distance(to) > SNAP_DISTANCE && now - since < TELEPORT_HOLD {
                return None;
            }
            self.teleport_hold = None;
        }
        Some(sample)
    }
}

/// Most samples a sender can lose between two it delivers: over three
/// minutes at [`AVATAR_SEND_HZ`]. A sender counts up by one a sample, so one
/// further ahead was not counted.
const MAX_SEQ_GAP: u32 = 4096;

/// Where a sample's `seq` stands against the newest kept from its sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeqOrder {
    /// Ahead by 1 to [`MAX_SEQ_GAP`] (or the first): kept.
    Newer,
    /// A duplicate, or behind (reordered): dropped.
    Stale,
    /// Further ahead than any loss explains: refused. Kept, it would make
    /// every later honest sample look older, freezing the sender's replica
    /// while everyone else saw it move.
    Jump,
}

/// [`SeqOrder`] of `seq` against `newest` (`seq` wraps, so the compare is a
/// wrapping one).
fn seq_order(seq: u32, newest: Option<u32>) -> SeqOrder {
    let Some(newest) = newest else { return SeqOrder::Newer };
    match seq.wrapping_sub(newest) {
        0 => SeqOrder::Stale,
        ahead if ahead <= MAX_SEQ_GAP => SeqOrder::Newer,
        ahead if ahead > u32::MAX / 2 => SeqOrder::Stale,
        _ => SeqOrder::Jump,
    }
}

/// A replica requested from the avatar runtime and not yet seen.
struct InFlight {
    peer: PeerId,
    /// Where it was asked to spawn.
    at: Vec3,
    since: f64,
    /// The look it is built with.
    look: AvatarDescriptor,
}

/// Replicas rebuilt for a changed look per second, across every peer, so
/// changing looks cannot hold up anyone's first spawn.
const REBUILDS_PER_SECOND: usize = 2;

/// Every other participant's avatar, as this machine knows it.
#[derive(Resource, Default)]
pub struct RemoteAvatars {
    peers: HashMap<PeerId, Remote>,
    in_flight: Option<InFlight>,
    /// When each rebuild of the last second started.
    rebuilds: VecDeque<f64>,
}

impl RemoteAvatars {
    fn add_peer(&mut self, peer: PeerId, name: String) {
        self.peers.entry(peer).or_insert(Remote {
            name,
            appearance: None,
            latest: None,
            received_at: 0.0,
            applied_jumps: None,
            entity: None,
            respawn: false,
            previous_look: None,
            placed: None,
            teleport_hold: None,
        });
    }

    /// Forget a peer; returns its replica for the caller to despawn. One
    /// still being built is despawned when it appears ([`tag_replicas`]).
    fn remove_peer(&mut self, peer: PeerId) -> Option<Entity> {
        self.peers.remove(&peer).and_then(|r| r.entity)
    }

    /// A replica already in the world with another look is rebuilt; the old
    /// body stays until the new one replaces it.
    fn set_appearance(&mut self, peer: PeerId, descriptor: AvatarDescriptor) {
        if let Some(r) = self.peers.get_mut(&peer) {
            if r.appearance.as_ref() == Some(&descriptor) {
                return;
            }
            if let Some(old) = r.appearance.replace(descriptor) {
                if r.entity.is_some() {
                    r.previous_look.get_or_insert(old);
                    r.respawn = true;
                }
            }
        }
    }

    /// Where a host script moved this peer's replica, while its samples
    /// still show the old place ([`TELEPORT_HOLD`]).
    fn teleported_to(&self, peer: PeerId) -> Option<Vec3> {
        self.peers.get(&peer)?.teleport_hold.map(|(to, _)| to)
    }

    /// Keep the newest sample ([`seq_order`]); true when it was kept. Every
    /// replica's spawn point, pose and intent come from the sample kept here,
    /// so only a finite one within [`WORLD_LIMIT`] is kept.
    fn accept_frame(&mut self, frame: AvatarFrame, now: f64) -> bool {
        if !frame.is_finite() || Vec3::from_array(frame.position).abs().max_element() >= WORLD_LIMIT {
            return false;
        }
        let Some(r) = self.peers.get_mut(&frame.peer) else { return false };
        if seq_order(frame.seq, r.latest.map(|f| f.seq)) != SeqOrder::Newer {
            return false;
        }
        r.latest = Some(frame);
        r.received_at = now;
        true
    }

    /// Where a sample from `peer` stands against its newest kept one.
    fn seq_order(&self, peer: PeerId, seq: u32) -> SeqOrder {
        seq_order(seq, self.peers.get(&peer).and_then(|r| r.latest).map(|f| f.seq))
    }

    /// Names of everyone this machine knows about.
    pub fn names(&self) -> Vec<(PeerId, String)> {
        self.peers.iter().map(|(p, r)| (*p, r.name.clone())).collect()
    }
}

/// The local avatar's outgoing state.
#[derive(Resource, Default)]
pub struct LocalAvatarNet {
    seq: u32,
    jumps: u8,
    direction: Vec3,
    sprint: bool,
    crouch: bool,
    accum: f32,
    announced: Option<Entity>,
    /// Where the host says the local avatar is, after it refused its
    /// movement ([`ToPlayer::Correct`]), not yet applied.
    correction: Option<Vec3>,
    /// Where the local avatar was at the end of last frame.
    placed: Option<Vec3>,
    /// When something on this machine last moved the local avatar farther
    /// than a step (the host's teleport, replicated; a respawn).
    moved_at: Option<f64>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Plugin
// ─────────────────────────────────────────────────────────────────────────────

/// Adds the session systems. Both shells add it; a session starts when the
/// shell inserts a [`NetLink`] with a [`HostSession`] or a [`PlayerSession`].
pub struct NetPlugin;

fn in_session(link: Option<Res<NetLink>>) -> bool {
    link.is_some()
}
fn no_session(link: Option<Res<NetLink>>) -> bool {
    link.is_none()
}
fn hosting(link: Option<Res<NetLink>>, host: Option<Res<HostSession>>) -> bool {
    link.is_some() && host.is_some()
}
fn joined(link: Option<Res<NetLink>>, player: Option<Res<PlayerSession>>) -> bool {
    link.is_some() && player.is_some()
}
/// Avian's spatial queries are available. Every shell that hosts has them;
/// one without would hold every player to [`PaceLimit::before_replica`].
fn physics_present(trees: Option<Res<ColliderTrees>>) -> bool {
    trees.is_some()
}

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<NetNotice>()
            .add_message::<WorldDownloaded>()
            .add_message::<LocalWorldReady>()
            .add_message::<SendChat>()
            .add_message::<EndSession>()
            .add_message::<PromptPurchase>()
            .add_message::<ClosePurchase>()
            .add_message::<SendReceipts>()
            .add_message::<SendWorld>()
            .add_message::<SendMotion>()
            .add_message::<SendRemoteReply>()
            .add_message::<RemoteArrived>()
            .add_message::<WorldArrived>()
            .add_message::<MotionArrived>()
            .add_message::<RemoteReplied>()
            .add_message::<FireRemote>()
            .add_message::<SendTracks>()
            .add_message::<TracksArrived>()
            .add_message::<AdmissionVerdict>()
            // Idempotent; the avatar runtime registers it too. Registered here
            // so the replica spawner's writer is valid in any shell.
            .add_message::<SpawnAvatar>()
            .init_resource::<RemoteAvatars>()
            .init_resource::<LocalAvatarNet>()
            .init_resource::<PeerPaces>()
            .init_resource::<PeerInputs>();

        // Every system is gated on a session existing, and Bevy evaluates run
        // conditions before validating parameters, so a shell that never
        // starts a session (or has no avatar runtime, like the headless
        // engine) pays nothing and never trips a missing-resource error.
        app.add_systems(
            Update,
            (
                host_pump.run_if(hosting),
                host_admissions.run_if(hosting),
                player_pump.run_if(joined),
                handle_shell_requests.run_if(in_session),
                host_send_chunks.run_if(hosting),
                host_send_replication.run_if(hosting),
                player_send_remotes.run_if(joined),
                player_send_tracks.run_if(joined),
                send_local_input.run_if(joined),
                player_timeouts.run_if(joined),
                announce_local_appearance.run_if(in_session),
                spawn_replicas.run_if(in_session),
            )
                .chain()
                .before(AvatarSystems::Lifecycle),
        )
        .add_systems(
            Update,
            tag_replicas
                .run_if(in_session)
                .after(AvatarSystems::Lifecycle)
                .before(AvatarSystems::Input),
        )
        .add_systems(
            Update,
            (capture_local_intent.run_if(in_session), drive_replica_intent.run_if(in_session))
                .after(AvatarSystems::Input)
                .before(AvatarSystems::Locomotion),
        )
        .add_systems(
            Update,
            (
                (correct_replicas.run_if(in_session), track_paces.run_if(hosting).run_if(physics_present)).chain(),
                (apply_host_correction.run_if(joined), send_local_avatar.run_if(in_session)).chain(),
            )
                .after(AvatarSystems::Locomotion)
                .before(AvatarSystems::Animation),
        )
        .add_systems(Update, clear_replicas_after_session.run_if(no_session));
    }
}

/// Start hosting: the shell passes the link it got from the transport.
pub fn begin_host(commands: &mut Commands, link: NetLink, config: HostConfig) {
    commands.insert_resource(HostSession::new(config));
    commands.insert_resource(link);
}

/// Start joining: the shell passes the link it got from the transport, and
/// the player's identity ticket when it has one (see [`Hello::identity`]).
pub fn begin_join(commands: &mut Commands, link: NetLink, player_name: &str, identity: Option<String>) {
    commands.insert_resource(PlayerSession::new(player_name, identity));
    commands.insert_resource(link);
    // The host measures this player's movement against the Space's pace, so
    // its own avatar moves at that pace, not its account's.
    commands.insert_resource(SpaceOwnsMotion);
}

fn end_session(commands: &mut Commands) {
    // Removing the link drops it, which tells the transport to shut down
    // after any commands already queued.
    commands.remove_resource::<NetLink>();
    commands.remove_resource::<HostSession>();
    commands.remove_resource::<PlayerSession>();
    commands.remove_resource::<SpaceOwnsMotion>();
}

// ─────────────────────────────────────────────────────────────────────────────
// Host systems
// ─────────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn host_pump(
    mut commands: Commands,
    time: Res<Time>,
    link: Res<NetLink>,
    mut host: ResMut<HostSession>,
    mut remotes: ResMut<RemoteAvatars>,
    mut inputs: ResMut<PeerInputs>,
    mut notices: MessageWriter<NetNotice>,
    mut arrivals: MessageWriter<RemoteArrived>,
    mut tracks: MessageWriter<TracksArrived>,
    mut paces: ResMut<PeerPaces>,
    policy: Option<Res<SpaceCharacterPolicy>>,
) {
    let now = time.elapsed_secs_f64();
    // Each player's newest avatar sample this frame, read after the rest.
    let mut samples = HashMap::new();
    for _ in 0..MAX_EVENTS_PER_FRAME {
        let Some(event) = link.poll() else { break };
        match event {
            LinkEvent::Listening { port, cert_sha256 } => {
                host.port = port;
                host.pin = Some(cert_sha256);
                notices.write(NetNotice::Hosting { port, pin: cert_sha256 });
            }
            LinkEvent::Opened { conn, remote, datagrams } => {
                if !host.has_room() {
                    link.send(conn, &ToPlayer::Refused { reason: host.full_reason() });
                    link.close(conn, "full");
                    continue;
                }
                let peer = host.next_peer;
                host.next_peer += 1;
                host.peers.insert(conn, HostPeer::new(peer, remote, datagrams, now));
            }
            LinkEvent::Frame { conn, body } => match decode_body::<ToHost>(&body) {
                Ok(ToHost::Remote(call)) => host_remote(&link, &mut host, &mut arrivals, conn, call, false, now),
                Ok(ToHost::Input(frame)) => host_input(&host, &mut inputs, conn, &frame),
                Ok(ToHost::Tracks(ops)) => host_tracks(&link, &mut host, &mut tracks, conn, ops, now),
                Ok(ToHost::Avatar(frame)) => keep_newest(&mut samples, conn, frame),
                Ok(msg) => host_message(&mut commands, &link, &mut host, &mut remotes, &mut notices, conn, msg, now),
                Err(e) => {
                    // Logged here, before the connection goes away. A version
                    // mismatch used to die inside bincode and `kick` said
                    // nothing, so the host's log was empty.
                    let reason = peer_decode_reason(&e);
                    error!("net: {reason}");
                    let ready = host.peers.get(&conn).is_some_and(|p| p.stage == PeerStage::Ready);
                    if ready {
                        kick(&link, &mut host, conn, &reason);
                    } else {
                        refuse(&link, &mut host, conn, &reason);
                    }
                }
            },
            LinkEvent::Datagram { conn, bytes } => match decode_datagram(&bytes) {
                Ok(Datagram::Avatar(frame)) => keep_newest(&mut samples, conn, frame),
                Ok(Datagram::Input(frame)) => host_input(&host, &mut inputs, conn, &frame),
                Ok(Datagram::Remote(call)) => host_remote(&link, &mut host, &mut arrivals, conn, call, true, now),
                // Motion flows host to player only; an unreadable datagram is
                // dropped like a lost one.
                Ok(Datagram::Motion(_)) | Err(_) => {}
            },
            LinkEvent::Closed { conn, reason } => {
                if let Some(p) = host.peers.remove(&conn) {
                    inputs.by_peer.remove(&p.peer);
                    host.rates.forget_peer(p.peer);
                    if p.stage == PeerStage::Ready {
                        for (other, _) in host.ready() {
                            link.send(other, &ToPlayer::PeerLeft { peer: p.peer });
                        }
                        if let Some(e) = remotes.remove_peer(p.peer) {
                            commands.entity(e).try_despawn();
                        }
                        notices.write(NetNotice::PeerLeft { peer: p.peer, name: p.name.clone() });
                    }
                    info!("net: {} ({}) left: {reason}", p.name, p.remote);
                }
            }
            LinkEvent::Failed { reason } => {
                error!("net: host stopped: {reason}");
                notices.write(NetNotice::HostEnded { reason });
                end_session(&mut commands);
                return;
            }
        }
    }
    // The host's own Space says whether movement is checked; nothing a
    // player sends can.
    let checked = policy.as_deref().map_or(true, SpaceCharacterPolicy::movement_check);
    for (conn, frame) in samples {
        host_avatar_frame(&link, &mut host, &mut remotes, &mut paces, checked, conn, frame, now);
    }
    // Looks that waited their turn.
    let due: Vec<ConnId> = host
        .peers
        .iter()
        .filter(|(_, p)| {
            p.stage == PeerStage::Ready && p.pending_appearance.is_some() && now - p.appearance_at >= APPEARANCE_INTERVAL
        })
        .map(|(conn, _)| *conn)
        .collect();
    for conn in due {
        if let Some((desc, clean)) = host.peers.get_mut(&conn).and_then(|p| p.pending_appearance.take()) {
            take_appearance(&link, &mut host, &mut remotes, conn, desc, clean, now);
        }
    }
}

/// Keep a player's sample if it is the newest of those that arrived this
/// frame ([`seq_order`]). A burst, such as a
/// stall's backlog on a stream transport, is read as its last sample: older
/// ones never spend the player's rate, and no stale place is relayed.
fn keep_newest(samples: &mut HashMap<ConnId, AvatarFrame>, conn: ConnId, frame: AvatarFrame) {
    if seq_order(frame.seq, samples.get(&conn).map(|kept| kept.seq)) == SeqOrder::Newer {
        samples.insert(conn, frame);
    }
}

/// Take a player's changed look: keep it, tell everyone else, and rebuild
/// its replica.
fn take_appearance(
    link: &NetLink,
    host: &mut HostSession,
    remotes: &mut RemoteAvatars,
    conn: ConnId,
    desc: AvatarDescriptor,
    clean: String,
    now: f64,
) {
    let Some(p) = host.peers.get_mut(&conn) else { return };
    p.appearance = Some(clean.clone());
    p.appearance_at = now;
    p.pending_appearance = None;
    let peer = p.peer;
    for (other, _) in host.ready() {
        if other != conn {
            link.send(other, &ToPlayer::Appearance { peer, descriptor_json: clean.clone() });
        }
    }
    remotes.set_appearance(peer, desc);
}

#[allow(clippy::too_many_arguments)]
fn host_message(
    commands: &mut Commands,
    link: &NetLink,
    host: &mut HostSession,
    remotes: &mut RemoteAvatars,
    notices: &mut MessageWriter<NetNotice>,
    conn: ConnId,
    msg: ToHost,
    now: f64,
) {
    let Some(stage) = host.peers.get(&conn).map(|p| p.stage) else { return };
    match msg {
        ToHost::Hello(hello) => {
            if stage != PeerStage::AwaitingHello {
                return kick(link, host, conn, "sent Hello twice");
            }
            if hello.protocol != PROTOCOL_VERSION {
                link.send(
                    conn,
                    &ToPlayer::Refused {
                        reason: format!(
                            "This host speaks protocol {PROTOCOL_VERSION} and your Player speaks {}. \
                             Update whichever is older.",
                            hello.protocol
                        ),
                    },
                );
                link.close(conn, "protocol mismatch");
                host.peers.remove(&conn);
                return;
            }
            let peer = host.peers[&conn].peer;
            let name = host.unique_name(&sanitize_name(&hello.name), peer);
            let admission = host.admission;
            let p = host.peers.get_mut(&conn).expect("present above");
            p.name = name.clone();
            p.identity = hello.identity.as_deref().and_then(sanitize_ticket).map(Ticket);
            // The player's own words, so printable and short before logging.
            let app: String = hello.engine_version.chars().filter(|c| !c.is_control()).take(64).collect();
            info!("net: {name} ({}) said Hello as peer {peer}, running {}", p.remote, app.trim());
            if admission {
                // The gallery lists this session: nothing of the world until
                // it vouches for this player (`host_admissions`).
                p.stage = PeerStage::AwaitingAdmission;
                p.admit_by = now + ADMISSION_TIMEOUT;
                notices.write(NetNotice::Admit { peer, ticket: p.identity.clone() });
            } else {
                welcome_peer(link, host, conn, now);
            }
        }
        ToHost::RequestChunks { hashes } => {
            if stage == PeerStage::AwaitingHello {
                return kick(link, host, conn, "requested the world before Hello");
            }
            if stage == PeerStage::AwaitingAdmission {
                return kick(link, host, conn, "requested the world before the gallery admitted it");
            }
            let mut seen = BTreeSet::new();
            let mut queue = Vec::new();
            let mut missing = Vec::new();
            for h in hashes.into_iter().take(eustress_echk::MAX_CHUNKS) {
                if !seen.insert(h.clone()) {
                    continue;
                }
                if host.config.chunks.contains_key(&h) {
                    queue.push((h, 0usize));
                } else {
                    missing.push(h);
                }
            }
            for hash in missing {
                link.send(conn, &ToPlayer::ChunkMissing { hash });
            }
            if let Some(p) = host.peers.get_mut(&conn) {
                p.outbox.extend(queue);
            }
        }
        ToHost::WorldReady => {
            if stage != PeerStage::Downloading {
                return;
            }
            let spawn = Vec3::from_array(host.spawn_for(host.peers[&conn].peer));
            let (peer, name, appearance, identity) = {
                let p = host.peers.get_mut(&conn).expect("present above");
                p.stage = PeerStage::Ready;
                // Its avatar starts where the Welcome put it, so its first
                // sample is measured from there like any other.
                p.travel = Some(TravelBudget::start(now, spawn, PaceLimit::before_replica()));
                // A look that waited while it downloaded is its newest.
                if let Some((_, waiting)) = p.pending_appearance.take() {
                    p.appearance = Some(waiting);
                }
                if p.appearance.is_some() {
                    p.appearance_at = now;
                }
                (p.peer, p.name.clone(), p.appearance.clone(), p.identity.take())
            };
            // Tell the newcomer who is here: the host, then every other player.
            link.send(conn, &ToPlayer::PeerJoined(PeerInfo { peer: HOST_PEER, name: host.config.host_name.clone() }));
            if let Some(json) = &host.appearance {
                link.send(conn, &ToPlayer::Appearance { peer: HOST_PEER, descriptor_json: json.clone() });
            }
            for (other_conn, other) in host.ready() {
                if other_conn == conn {
                    continue;
                }
                link.send(conn, &ToPlayer::PeerJoined(PeerInfo { peer: other.peer, name: other.name.clone() }));
                if let Some(json) = &other.appearance {
                    link.send(conn, &ToPlayer::Appearance { peer: other.peer, descriptor_json: json.clone() });
                }
                // And tell everyone else about the newcomer.
                link.send(other_conn, &ToPlayer::PeerJoined(PeerInfo { peer, name: name.clone() }));
                if let Some(json) = &appearance {
                    link.send(other_conn, &ToPlayer::Appearance { peer, descriptor_json: json.clone() });
                }
            }
            remotes.add_peer(peer, name.clone());
            if let Some(desc) = appearance.as_deref().and_then(parse_appearance) {
                remotes.set_appearance(peer, desc);
            }
            notices.write(NetNotice::PeerJoined { peer, name });
            if let Some(ticket) = identity {
                notices.write(NetNotice::PeerIdentity { peer, ticket });
            }
        }
        ToHost::Appearance { descriptor_json } => {
            if stage == PeerStage::AwaitingHello {
                return kick(link, host, conn, "sent an appearance before Hello");
            }
            // A Player sends it after its Welcome; before admission it is
            // dropped, not kept for a player who may be refused.
            if stage == PeerStage::AwaitingAdmission {
                return;
            }
            let Some((desc, clean)) = clean_appearance(&descriptor_json) else {
                warn!("net: ignored an invalid appearance from connection {conn}");
                return;
            };
            let p = host.peers.get_mut(&conn).expect("present above");
            // The same look again changes nothing and costs nothing, and
            // replaces any change still waiting.
            if p.appearance.as_deref() == Some(clean.as_str()) {
                p.pending_appearance = None;
                return;
            }
            if p.pending_appearance.as_ref().is_some_and(|(_, waiting)| *waiting == clean) {
                return;
            }
            // One changed look per APPEARANCE_INTERVAL; a sooner one waits
            // (the newest wins) and counts toward removing the player, so a
            // flood of looks removes it, downloading or in the world.
            if now - p.appearance_at < APPEARANCE_INTERVAL {
                p.pending_appearance = Some((desc, clean));
                if p.violation(now) {
                    return kick(link, host, conn, "appearance changes the host could not accept");
                }
                return;
            }
            // Downloading: nobody sees it before Ready, so nothing is rebuilt.
            if stage != PeerStage::Ready {
                p.appearance = Some(clean);
                p.appearance_at = now;
                return;
            }
            take_appearance(link, host, remotes, conn, desc, clean, now);
        }
        ToHost::Chat { text } => {
            // A guest's chat goes nowhere (GUEST_PLAY.md section 8).
            if stage != PeerStage::Ready || host.peers.get(&conn).is_some_and(|p| p.guest) {
                return;
            }
            let Some(text) = sanitize_chat(&text) else { return };
            let (peer, name, allowed) = {
                let p = host.peers.get_mut(&conn).expect("present above");
                while p.chat_times.front().is_some_and(|t| now - t > CHAT_WINDOW) {
                    p.chat_times.pop_front();
                }
                let allowed = p.chat_times.len() < CHAT_BURST;
                if allowed {
                    p.chat_times.push_back(now);
                }
                (p.peer, p.name.clone(), allowed)
            };
            if !allowed {
                return;
            }
            for (other, _) in host.ready() {
                link.send(other, &ToPlayer::Chat { peer, name: name.clone(), text: text.clone() });
            }
            notices.write(NetNotice::Chat { peer, name, text });
        }
        ToHost::Goodbye => {
            link.close(conn, "left");
        }
        ToHost::PurchaseClosed { prompt, purchased } => {
            if stage != PeerStage::Ready {
                return;
            }
            let p = host.peers.get_mut(&conn).expect("present above");
            // Only an answer to a prompt this host sent this player counts.
            if p.prompts.remove(&prompt) {
                notices.write(NetNotice::PurchaseClosed { peer: p.peer, prompt, purchased });
            }
        }
        ToHost::Receipts { purchase_ids } => {
            if stage != PeerStage::Ready {
                return;
            }
            let p = host.peers.get_mut(&conn).expect("present above");
            while p.receipt_times.front().is_some_and(|t| now - t > RECEIPT_WINDOW) {
                p.receipt_times.pop_front();
            }
            if p.receipt_times.len() >= RECEIPT_BURST {
                return;
            }
            p.receipt_times.push_back(now);
            let purchase_ids = sanitize_receipts(&purchase_ids);
            if !purchase_ids.is_empty() {
                notices.write(NetNotice::Receipts { peer: p.peer, purchase_ids });
            }
        }
        // Routed by `host_pump` before they reach here.
        ToHost::Remote(_) | ToHost::Input(_) | ToHost::Tracks(_) | ToHost::Avatar(_) => {}
    }
    let _ = commands;
}

/// Pass a player's track changes to the shell. A message over the size
/// limit counts toward removing the player; the shell checks each change and
/// the player's rate.
fn host_tracks(
    link: &NetLink,
    host: &mut HostSession,
    tracks: &mut MessageWriter<TracksArrived>,
    conn: ConnId,
    ops: Vec<TrackWire>,
    now: f64,
) {
    let Some(p) = host.peers.get_mut(&conn).filter(|p| p.stage == PeerStage::Ready) else { return };
    if ops.len() > MAX_TRACK_OPS_PER_MESSAGE {
        if p.violation(now) {
            return kick(link, host, conn, "animation changes the host could not accept");
        }
        return;
    }
    if !ops.is_empty() {
        tracks.write(TracksArrived { peer: p.peer, ops });
    }
}

/// Keep a player's input samples for anything on the host to read.
fn host_input(host: &HostSession, inputs: &mut PeerInputs, conn: ConnId, frame: &InputFrame) {
    let Some(p) = host.peers.get(&conn).filter(|p| p.stage == PeerStage::Ready) else { return };
    inputs.by_peer.entry(p.peer).or_default().accept(frame);
}

/// Pass a player's remote call to the shell when it is bounded and within
/// the player's rate. Malformed calls count toward removing the player; a
/// call over the rate is only dropped, since a busy script causes those.
fn host_remote(
    link: &NetLink,
    host: &mut HostSession,
    arrivals: &mut MessageWriter<RemoteArrived>,
    conn: ConnId,
    call: RemoteCall,
    unreliable: bool,
    now: f64,
) {
    let Some(p) = host.peers.get_mut(&conn).filter(|p| p.stage == PeerStage::Ready) else { return };
    let peer = p.peer;
    if let Err(e) = check_call(&call, &ValueLimits::default()) {
        debug!("net: refused a remote call from {}: {e}", p.name);
        if p.violation(now) {
            return kick(link, host, conn, "remote calls the host could not accept");
        }
        return;
    }
    if !host.rates.allow(peer, call.remote, now) {
        return;
    }
    arrivals.write(RemoteArrived { peer, call, unreliable });
}

/// Send what the host shell replicated: world frames to one player each (a
/// player's catch-up first), motion to everyone in the world, and answers to
/// invocations.
fn host_send_replication(
    link: Res<NetLink>,
    mut host: ResMut<HostSession>,
    mut worlds: MessageReader<SendWorld>,
    mut motion: MessageReader<SendMotion>,
    mut replies: MessageReader<SendRemoteReply>,
) {
    for msg in worlds.read() {
        deliver_world(&link, &mut host, msg);
    }
    for msg in motion.read() {
        for (conn, p) in host.ready() {
            for frame in &msg.frames {
                if p.datagrams {
                    link.datagram(conn, encode_datagram(&Datagram::Motion(frame.clone())));
                } else {
                    link.send(conn, &ToPlayer::Motion(frame.clone()));
                }
            }
        }
    }
    for msg in replies.read() {
        if let Some((conn, _)) = host.ready().find(|(_, p)| p.peer == msg.peer) {
            link.send(conn, &ToPlayer::RemoteReply(msg.reply.clone()));
        }
    }
}

/// Send one player's world frames, holding live ones back until its
/// catch-up has gone. Returns the frames sent.
fn deliver_world(link: &NetLink, host: &mut HostSession, msg: &SendWorld) -> usize {
    let Some((conn, p)) = host.peers.iter_mut().find(|(_, p)| p.peer == msg.peer && p.stage == PeerStage::Ready) else {
        return 0;
    };
    if !msg.catch_up && !p.caught_up {
        return 0; // its catch-up, still to come, includes this
    }
    p.caught_up = true;
    for frame in &msg.frames {
        link.send(*conn, &ToPlayer::World(frame.clone()));
    }
    msg.frames.len()
}

/// Validate a player's avatar sample, keep it, and relay it to everyone else.
/// It must be finite, inside the world, and within its [`TravelBudget`] at
/// its replica's [`Pace`] ([`PaceLimit::before_replica`] until the replica
/// exists). A seat's vehicle, a host script's teleport and a carrier a
/// script moved all move a player without it walking, so none is counted
/// against it. A refused sample counts toward removing the player, and the
/// player hears where the host has it ([`ToPlayer::Correct`]), at most once
/// every [`CORRECTION_INTERVAL`]. A seated player's samples are kept and
/// relayed at its seat, wherever they say it is. Samples past
/// [`MAX_SAMPLES_PER_SECOND`] are dropped unread, older ones are dropped, one
/// further ahead than [`MAX_SEQ_GAP`] is refused, and only a sample kept here
/// is relayed.
#[allow(clippy::too_many_arguments)]
fn host_avatar_frame(
    link: &NetLink,
    host: &mut HostSession,
    remotes: &mut RemoteAvatars,
    paces: &mut PeerPaces,
    checked: bool,
    conn: ConnId,
    mut frame: AvatarFrame,
    now: f64,
) {
    let Some(p) = host.peers.get_mut(&conn) else { return };
    if p.stage != PeerStage::Ready || !p.samples.take(now) {
        return;
    }
    // An old or repeated sample says nothing new, and is neither measured
    // nor relayed; one further ahead than any loss explains is refused.
    match remotes.seq_order(p.peer, frame.seq) {
        SeqOrder::Newer => {}
        SeqOrder::Stale => return,
        SeqOrder::Jump => {
            if p.violation(now) {
                return kick(link, host, conn, "movement the host could not accept");
            }
            return;
        }
    }
    let pos = Vec3::from_array(frame.position);
    let mut at_seat = None;
    let plausible = frame.is_finite() && pos.abs().max_element() < WORLD_LIMIT && {
        let pace = paces.pace(p.peer);
        let limit = pace.map_or_else(PaceLimit::before_replica, |pace| pace.limit);
        let teleported = remotes.teleported_to(p.peer).is_some_and(|to| pos.distance(to) <= SNAP_DISTANCE);
        let followed = pace
            .and_then(|pace| pace.carried_jump)
            .zip(p.travel)
            .is_some_and(|((by, _), travel)| pos.distance(travel.pos + by) <= SNAP_DISTANCE);
        if let Some(seat) = pace.filter(|pace| pace.seated).map(|pace| pace.place) {
            // The seat carries it, and its samples do not move its replica:
            // the budget waits where the host has it, so leaving the seat
            // starts from there, not from wherever the samples said.
            p.travel = Some(TravelBudget::start(now, seat, limit));
            at_seat = Some(seat);
            true
        } else if !checked {
            // The Space turned the check off (StarterPlayer
            // ServerMovementCheck): kept wherever it is, and measured from
            // here should the Space turn it on again.
            p.travel = Some(TravelBudget::start(now, pos, limit));
            true
        } else if teleported || followed || p.travel.is_none() {
            // Moved by the host, once: the budget starts again where it went.
            // (Never without a budget otherwise: Ready starts one at the
            // spawn point.)
            if followed {
                paces.followed_carrier(p.peer);
            }
            p.travel = Some(TravelBudget::start(now, pos, limit));
            true
        } else {
            p.travel.as_mut().is_some_and(|travel| travel.spend(now, pos, limit))
        }
    };
    if !plausible {
        // Where the host has it, so it walks on from there rather than
        // being refused sample after sample.
        if let Some(at) = p.travel.map(|travel| travel.pos).filter(|_| now - p.corrected_at >= CORRECTION_INTERVAL) {
            p.corrected_at = now;
            link.send(conn, &ToPlayer::Correct { position: at.to_array() });
        }
        if p.violation(now) {
            let name = p.name.clone();
            warn!("net: removing {name}: {MAX_VIOLATIONS} impossible movements in {VIOLATION_WINDOW} s");
            return kick(link, host, conn, "movement the host could not accept");
        }
        return;
    }
    frame.peer = p.peer; // never trust the sender's claim of who it is
    // Seated, it is where its seat is: its replica leaves the seat from
    // there, a new body for a changed look spawns there, and everyone else
    // sees it there.
    if let Some(seat) = at_seat {
        frame.position = seat.to_array();
    }
    // Only a sample kept here is relayed: everyone sees what the host has.
    if !remotes.accept_frame(frame, now) {
        return;
    }

    let bytes = encode_datagram(&Datagram::Avatar(frame));
    for (other, o) in host.ready() {
        if other == conn {
            continue;
        }
        if o.datagrams {
            link.datagram(other, bytes.clone());
        } else {
            link.send(other, &ToPlayer::Avatar(frame));
        }
    }
}

/// The words for a frame this host could not read. A version mismatch is
/// already a sentence; anything else keeps the "unreadable message" prefix,
/// and a bare bincode trailing-bytes failure is said as a version mismatch.
fn peer_decode_reason(err: &WireError) -> String {
    match err {
        WireError::VersionMismatch { .. } => err.to_string(),
        other => explain_transport_failure(&format!("unreadable message: {other}")),
    }
}

fn kick(link: &NetLink, host: &mut HostSession, conn: ConnId, reason: &str) {
    link.send(conn, &ToPlayer::Kicked { reason: reason.to_string() });
    link.close(conn, reason);
    // The transport reports Closed, which cleans up and tells everyone.
    if let Some(p) = host.peers.get_mut(&conn) {
        p.outbox.clear();
    }
}

/// Welcome a player who said `Hello`: the world's manifest, where it spawns,
/// the session's tick. From now on it downloads.
fn welcome_peer(link: &NetLink, host: &mut HostSession, conn: ConnId, now: f64) {
    let Some(peer) = host.peers.get(&conn).map(|p| p.peer) else { return };
    let welcome = Welcome {
        protocol: PROTOCOL_VERSION,
        peer,
        host_name: host.config.host_name.clone(),
        world: host.config.world.clone(),
        spawn: host.spawn_for(peer),
        max_players: host.config.max_players,
        sim_id: host.config.sim_id.clone(),
        host_identity: host.identity.clone(),
        tick: tick_at(now),
    };
    if let Some(p) = host.peers.get_mut(&conn) {
        p.stage = PeerStage::Downloading;
    }
    link.send(conn, &ToPlayer::Welcome(welcome));
}

/// Turn a player away before it has anything of the world, with the reason
/// it is shown. Nobody else ever heard of it.
fn refuse(link: &NetLink, host: &mut HostSession, conn: ConnId, reason: &str) {
    link.send(conn, &ToPlayer::Refused { reason: reason.to_string() });
    link.close(conn, reason);
    host.peers.remove(&conn);
}

/// The gallery's answers to [`NetNotice::Admit`]: an admitted player is
/// welcomed, a refused one turned away with the gallery's reason, and one
/// the gallery has not answered for within [`ADMISSION_TIMEOUT`] turned away
/// as unconfirmed.
fn host_admissions(
    time: Res<Time>,
    link: Res<NetLink>,
    mut host: ResMut<HostSession>,
    mut verdicts: MessageReader<AdmissionVerdict>,
) {
    let now = time.elapsed_secs_f64();
    for verdict in verdicts.read() {
        let waiting = host
            .peers
            .iter()
            .find(|(_, p)| p.peer == verdict.peer && p.stage == PeerStage::AwaitingAdmission)
            .map(|(conn, _)| *conn);
        let Some(conn) = waiting else { continue };
        match &verdict.outcome {
            Ok(admitted) => {
                let p = host.peers.get_mut(&conn).expect("found above");
                p.admitted = Some(admitted.ticket_id.clone());
                p.guest = admitted.guest;
                info!("net: the gallery admitted {} as peer {}{}", p.name, p.peer, if p.guest { ", a guest" } else { "" });
                welcome_peer(&link, &mut host, conn, now);
            }
            Err(reason) => {
                info!("net: the gallery refused peer {}: {reason}", verdict.peer);
                refuse(&link, &mut host, conn, reason);
            }
        }
    }
    let late: Vec<ConnId> = host
        .peers
        .iter()
        .filter(|(_, p)| p.stage == PeerStage::AwaitingAdmission && now >= p.admit_by)
        .map(|(conn, _)| *conn)
        .collect();
    for conn in late {
        info!("net: no word from the gallery on peer {} in {ADMISSION_TIMEOUT} s", host.peers[&conn].peer);
        refuse(&link, &mut host, conn, ADMISSION_UNCONFIRMED);
    }
}

/// Feed each downloading player its world, a bounded amount per frame.
fn host_send_chunks(link: Res<NetLink>, mut host: ResMut<HostSession>) {
    let host = &mut *host;
    for (conn, p) in host.peers.iter_mut() {
        let mut budget = CHUNK_BUDGET_PER_FRAME;
        while budget > 0 {
            let Some((hash, offset)) = p.outbox.front().cloned() else { break };
            let Some(chunk) = host.config.chunks.get(&hash) else {
                p.outbox.pop_front();
                continue;
            };
            let end = (offset + CHUNK_PIECE_BYTES).min(chunk.len());
            link.send(
                *conn,
                &ToPlayer::ChunkPiece {
                    hash: hash.clone(),
                    offset: offset as u64,
                    total: chunk.len() as u64,
                    bytes: chunk[offset..end].to_vec(),
                },
            );
            budget = budget.saturating_sub(end - offset);
            if end >= chunk.len() {
                p.outbox.pop_front();
            } else if let Some(front) = p.outbox.front_mut() {
                front.1 = end;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Player systems
// ─────────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn player_pump(
    mut commands: Commands,
    time: Res<Time>,
    link: Res<NetLink>,
    mut player: ResMut<PlayerSession>,
    mut remotes: ResMut<RemoteAvatars>,
    cache: Option<Res<ChunkCacheRes>>,
    mut notices: MessageWriter<NetNotice>,
    mut downloaded: MessageWriter<WorldDownloaded>,
    (mut worlds, mut motions, mut replies): (
        MessageWriter<WorldArrived>,
        MessageWriter<MotionArrived>,
        MessageWriter<RemoteReplied>,
    ),
    app: Option<Res<AppVersion>>,
    mut local: ResMut<LocalAvatarNet>,
) {
    let now = time.elapsed_secs_f64();
    if player.stage_since.is_none() {
        player.stage_since = Some(now);
    }
    for _ in 0..MAX_EVENTS_PER_FRAME {
        let Some(event) = link.poll() else { break };
        let outcome = match event {
            LinkEvent::Opened { datagrams, .. } => {
                player.datagrams = datagrams;
                let engine_version = app.as_ref().map_or_else(
                    || concat!("eustress-networking ", env!("CARGO_PKG_VERSION")).to_string(),
                    |a| a.0.clone(),
                );
                link.send(HOST_CONN, &ToHost::Hello(player.hello(engine_version)));
                player.set_stage(JoinStage::AwaitingWelcome);
                Ok(())
            }
            LinkEvent::Frame { body, .. } => match decode_body::<ToPlayer>(&body) {
                Ok(ToPlayer::World(frame)) => {
                    // Only once the world it changes is open.
                    if matches!(player.stage, JoinStage::Opening | JoinStage::Playing) {
                        player.clock.observe(frame.tick, now);
                        worlds.write(WorldArrived(frame));
                    }
                    Ok(())
                }
                Ok(ToPlayer::Motion(frame)) => {
                    player_motion(&mut player, &mut motions, frame, now);
                    Ok(())
                }
                Ok(ToPlayer::RemoteReply(reply)) => {
                    replies.write(RemoteReplied(reply));
                    Ok(())
                }
                Ok(ToPlayer::Correct { position }) => {
                    if let Some(to) = usable_correction(position).filter(|_| player.stage == JoinStage::Playing) {
                        local.correction = Some(to);
                    }
                    Ok(())
                }
                Ok(msg) => player_message(
                    &mut commands,
                    &link,
                    &mut player,
                    &mut remotes,
                    cache.as_deref(),
                    &mut notices,
                    &mut downloaded,
                    msg,
                    now,
                ),
                Err(e) => Err(peer_decode_reason(&e)),
            },
            LinkEvent::Datagram { bytes, .. } => {
                match decode_datagram(&bytes) {
                    Ok(Datagram::Avatar(frame)) => player_avatar_frame(&player, &mut remotes, frame, now),
                    Ok(Datagram::Motion(frame)) => player_motion(&mut player, &mut motions, frame, now),
                    // Input and remote calls flow player to host only.
                    Ok(Datagram::Input(_)) | Ok(Datagram::Remote(_)) | Err(_) => {}
                }
                Ok(())
            }
            LinkEvent::Closed { reason, .. } => {
                Err(explain_transport_failure(&format!("the host closed the connection: {reason}")))
            }
            LinkEvent::Failed { reason } => Err(reason),
            LinkEvent::Listening { .. } => Ok(()),
        };
        if let Err(reason) = outcome {
            let failed_before_playing = player.stage != JoinStage::Playing;
            error!("net: {reason}");
            notices.write(if failed_before_playing {
                NetNotice::JoinFailed { reason }
            } else {
                NetNotice::Disconnected { reason }
            });
            end_session(&mut commands);
            return;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn player_message(
    commands: &mut Commands,
    link: &NetLink,
    player: &mut PlayerSession,
    remotes: &mut RemoteAvatars,
    cache: Option<&ChunkCacheRes>,
    notices: &mut MessageWriter<NetNotice>,
    downloaded: &mut MessageWriter<WorldDownloaded>,
    msg: ToPlayer,
    now: f64,
) -> Result<(), String> {
    match msg {
        ToPlayer::Welcome(w) => {
            if player.stage != JoinStage::AwaitingWelcome {
                return Err("the host sent a second Welcome".into());
            }
            if w.protocol != PROTOCOL_VERSION {
                return Err(format!(
                    "this Player speaks protocol {PROTOCOL_VERSION} and the host speaks {}",
                    w.protocol
                ));
            }
            w.world.validate().map_err(|e| format!("the host's world was refused: {e}"))?;
            player.clock.observe(w.tick, now);
            player.peer = Some(w.peer);
            player.host_name = w.host_name.clone();
            player.spawn = w.spawn;
            player.sim_id = w
                .sim_id
                .filter(|id| !id.is_empty() && id.len() <= MAX_SIM_ID_CHARS && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'));
            player.host_identity = w.host_identity.as_deref().and_then(sanitize_ticket);
            player.sizes = w.world.all_chunks().map(|c| (c.blake3.clone(), c.size)).collect();
            player.total = w.world.download_bytes();

            // Anything already cached and still intact is not downloaded again.
            for hash in w.world.hashes() {
                let cached = cache
                    .and_then(|c| c.0.get(&hash))
                    .filter(|bytes| content_hash(bytes) == hash);
                match cached {
                    Some(bytes) => {
                        player.received += bytes.len() as u64;
                        player.have.insert(hash, Arc::new(bytes));
                    }
                    None => {
                        player.wanted.insert(hash);
                    }
                }
            }
            player.world = Some(w.world);
            info!(
                "net: welcomed by {} as peer {}; world {} bytes, {} already cached",
                player.host_name,
                w.peer,
                player.total,
                player.received
            );
            if player.wanted.is_empty() {
                finish_download(player, downloaded)?;
            } else {
                player.set_stage(JoinStage::Downloading);
                link.send(HOST_CONN, &ToHost::RequestChunks { hashes: player.wanted.iter().cloned().collect() });
            }
        }
        ToPlayer::Refused { reason } => return Err(format!("the host refused the join: {reason}")),
        ToPlayer::Kicked { reason } => return Err(format!("the host removed this player: {reason}")),
        ToPlayer::ChunkPiece { hash, offset, total, bytes } => {
            if !player.wanted.contains(&hash) {
                return Err("the host sent a chunk that was not asked for".into());
            }
            let expected = player.sizes.get(&hash).copied().unwrap_or(0);
            if total != expected || total > MAX_CHUNK_BYTES {
                return Err(format!("chunk {hash} is {total} bytes, the manifest said {expected}"));
            }
            let buf = player.partial.entry(hash.clone()).or_default();
            if offset != buf.len() as u64 || buf.len() as u64 + bytes.len() as u64 > total {
                return Err(format!("chunk {hash} arrived out of order"));
            }
            buf.extend_from_slice(&bytes);
            let complete = buf.len() as u64 == total;
            player.received += bytes.len() as u64;
            if complete {
                let data = player.partial.remove(&hash).unwrap_or_default();
                if content_hash(&data) != hash {
                    return Err(format!("chunk {hash} does not match its content hash"));
                }
                if let Some(c) = cache {
                    c.0.put(&hash, &data);
                }
                player.wanted.remove(&hash);
                player.have.insert(hash, Arc::new(data));
            }
            if now - player.last_progress > 0.25 || player.wanted.is_empty() {
                player.last_progress = now;
                notices.write(NetNotice::Downloading { received: player.received, total: player.total });
            }
            if player.wanted.is_empty() && player.stage == JoinStage::Downloading {
                finish_download(player, downloaded)?;
            }
        }
        ToPlayer::ChunkMissing { hash } => {
            return Err(format!("the host does not have chunk {hash} of its own world"));
        }
        ToPlayer::PeerJoined(info) => {
            if Some(info.peer) == player.peer {
                return Ok(());
            }
            let name = sanitize_name(&info.name);
            player.names.insert(info.peer, name.clone());
            remotes.add_peer(info.peer, name.clone());
            notices.write(NetNotice::PeerJoined { peer: info.peer, name });
        }
        ToPlayer::PeerLeft { peer } => {
            if let Some(e) = remotes.remove_peer(peer) {
                commands.entity(e).try_despawn();
            }
            let name = player.names.remove(&peer).unwrap_or_default();
            notices.write(NetNotice::PeerLeft { peer, name });
        }
        ToPlayer::Appearance { peer, descriptor_json } => {
            if Some(peer) != player.peer {
                if let Some(desc) = parse_appearance(&descriptor_json) {
                    remotes.set_appearance(peer, desc);
                }
            }
        }
        ToPlayer::Chat { peer, name, text } => {
            let name = sanitize_name(&name);
            if let Some(text) = sanitize_chat(&text) {
                notices.write(NetNotice::Chat { peer, name, text });
            }
        }
        ToPlayer::Avatar(frame) => player_avatar_frame(player, remotes, frame, now),
        ToPlayer::PurchasePrompt { prompt, product, expects } => {
            // Only in the world; the shell decides whether to offer it.
            if player.stage == JoinStage::Playing {
                notices.write(NetNotice::PurchasePrompt { prompt, product, expects });
            }
        }
        // Routed by `player_pump` before they reach here.
        ToPlayer::World(_) | ToPlayer::Motion(_) | ToPlayer::RemoteReply(_) | ToPlayer::Correct { .. } => {}
    }
    let _ = link;
    Ok(())
}

/// Hand the host's motion to the shell, once this player is in the world.
fn player_motion(player: &mut PlayerSession, motions: &mut MessageWriter<MotionArrived>, frame: MotionFrame, now: f64) {
    if player.stage != JoinStage::Playing {
        return;
    }
    player.clock.observe(frame.tick, now);
    motions.write(MotionArrived(frame));
}

/// Send the shell's remote calls to the host.
fn player_send_remotes(link: Res<NetLink>, player: Res<PlayerSession>, mut calls: MessageReader<FireRemote>) {
    for fire in calls.read() {
        if player.stage != JoinStage::Playing {
            continue;
        }
        if fire.unreliable && player.datagrams {
            let bytes = encode_datagram(&Datagram::Remote(fire.call.clone()));
            if !bytes.is_empty() && bytes.len() <= crate::repl::motion::MAX_MOTION_DATAGRAM {
                link.datagram(HOST_CONN, bytes);
            }
            // Too large for a datagram: an unreliable call may be lost anyway.
        } else {
            link.send(HOST_CONN, &ToHost::Remote(fire.call.clone()));
        }
    }
}

/// Send the shell's track changes to the host, in messages it accepts.
fn player_send_tracks(link: Res<NetLink>, player: Res<PlayerSession>, mut sends: MessageReader<SendTracks>) {
    for SendTracks(ops) in sends.read() {
        if player.stage != JoinStage::Playing {
            continue;
        }
        for chunk in ops.chunks(MAX_TRACK_OPS_PER_MESSAGE) {
            link.send(HOST_CONN, &ToHost::Tracks(chunk.to_vec()));
        }
    }
}

/// Sample this player's devices every tick and send the newest few samples.
#[allow(clippy::too_many_arguments)]
fn send_local_input(
    time: Res<Time>,
    link: Res<NetLink>,
    mut player: ResMut<PlayerSession>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    gamepads: Query<&Gamepad>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    if player.stage != JoinStage::Playing {
        return;
    }
    let period = (1.0 / TICK_HZ) as f32;
    player.input_accum += time.delta_secs();
    if player.input_accum < period {
        return;
    }
    player.input_accum = (player.input_accum - period).min(period);

    let now = time.elapsed_secs_f64();
    let mut s = InputSample { tick: player.clock.host_tick(now).map_or(0, |t| t as u64), ..Default::default() };
    if let Some(keys) = &keys {
        for k in keys.get_pressed() {
            if let Some(bit) = key_bit(*k) {
                s.set_key(bit);
            }
        }
    }
    if let Some(mouse) = &mouse {
        for (button, bit) in [(MouseButton::Left, MOUSE_LEFT), (MouseButton::Right, MOUSE_RIGHT), (MouseButton::Middle, MOUSE_MIDDLE)] {
            if mouse.pressed(button) {
                s.mouse |= bit;
            }
        }
    }
    if let Some(pad) = gamepads.iter().next() {
        for (i, button) in PAD_BUTTONS.iter().enumerate() {
            if pad.pressed(*button) {
                s.pad |= 1 << i;
            }
        }
        let axis = |a: GamepadAxis| (pad.get(a).unwrap_or(0.0).clamp(-1.0, 1.0) * 32767.0) as i16;
        s.axes[AXIS_LEFT_X] = axis(GamepadAxis::LeftStickX);
        s.axes[AXIS_LEFT_Y] = axis(GamepadAxis::LeftStickY);
        s.axes[AXIS_RIGHT_X] = axis(GamepadAxis::RightStickX);
        s.axes[AXIS_RIGHT_Y] = axis(GamepadAxis::RightStickY);
        let trigger = |b: GamepadButton| (pad.get(b).unwrap_or(0.0).clamp(0.0, 1.0) * 32767.0) as i16;
        s.axes[AXIS_LEFT_TRIGGER] = trigger(GamepadButton::LeftTrigger2);
        s.axes[AXIS_RIGHT_TRIGGER] = trigger(GamepadButton::RightTrigger2);
    }
    // The camera that draws last is the one the player looks through.
    if let Some((camera, tf)) = cameras.iter().filter(|(c, _)| c.is_active).max_by_key(|(c, _)| c.order) {
        s.camera = tf.translation().to_array();
        let look = tf.forward().as_vec3();
        s.look = encode_dir(look);
        let cursor = windows.iter().next().and_then(|w| w.cursor_position());
        let aim = cursor.and_then(|c| camera.viewport_to_world(tf, c).ok()).map(|ray| ray.direction.as_vec3());
        s.aim = encode_dir(aim.unwrap_or(look));
    }
    if !s.is_valid() {
        return;
    }

    player.input_sent.push_front(s);
    player.input_sent.truncate(INPUT_REDUNDANCY);
    let frame = InputFrame { samples: player.input_sent.iter().copied().collect() };
    if player.datagrams {
        link.datagram(HOST_CONN, encode_datagram(&Datagram::Input(frame)));
    } else {
        link.send(HOST_CONN, &ToHost::Input(frame));
    }
}

/// Gamepad buttons, by bit of [`InputSample::pad`].
const PAD_BUTTONS: [GamepadButton; 16] = [
    GamepadButton::South,
    GamepadButton::East,
    GamepadButton::North,
    GamepadButton::West,
    GamepadButton::LeftTrigger,
    GamepadButton::RightTrigger,
    GamepadButton::LeftTrigger2,
    GamepadButton::RightTrigger2,
    GamepadButton::Select,
    GamepadButton::Start,
    GamepadButton::LeftThumb,
    GamepadButton::RightThumb,
    GamepadButton::DPadUp,
    GamepadButton::DPadDown,
    GamepadButton::DPadLeft,
    GamepadButton::DPadRight,
];

/// Decode a world's chunks into records, checking every path, whichever way
/// the chunks arrived: from a host, or downloaded from R2 by a published
/// simulation's fetch. `chunk` returns a chunk's bytes by content hash; the
/// caller has already verified each against its hash.
pub fn assemble_world<'a>(
    world: &WorldManifest,
    chunk: impl Fn(&str) -> Option<&'a [u8]>,
    spawn: [f32; 3],
) -> Result<DownloadedWorld, String> {
    let decode = |hash: &str| -> Result<Vec<Record>, String> {
        let bytes = chunk(hash).ok_or_else(|| format!("chunk {hash} never arrived"))?;
        let records = decode_chunk(bytes).map_err(|e| format!("chunk {hash}: {e}"))?;
        for (path, _) in &records {
            if !is_safe_record_path(path) {
                return Err(format!("chunk {hash} holds an unsafe path {path:?}"));
            }
        }
        Ok(records)
    };

    let mut spaces = Vec::with_capacity(world.spaces.len());
    for space in &world.spaces {
        let mut records = Vec::new();
        for c in &space.chunks {
            records.extend(decode(&c.blake3)?);
        }
        spaces.push((space.name.clone(), records));
    }
    let mut assets = Vec::new();
    for c in &world.assets {
        for record in decode(&c.blake3)? {
            if !record.0.starts_with("assets/") {
                return Err(format!("asset chunk holds {:?}, outside assets/", record.0));
            }
            assets.push(record);
        }
    }
    Ok(DownloadedWorld {
        universe: world.universe.clone(),
        start_space: world.opening_space().map(|s| s.name.clone()).unwrap_or_default(),
        spaces,
        assets,
        spawn,
    })
}

/// Decode every chunk and hand the world to the shell.
fn finish_download(player: &mut PlayerSession, downloaded: &mut MessageWriter<WorldDownloaded>) -> Result<(), String> {
    let manifest = player.world.as_ref().ok_or("no world to finish")?;
    let have = &player.have;
    let world = assemble_world(manifest, |hash| have.get(hash).map(|b| b.as_slice()), player.spawn)?;
    player.have.clear();
    player.set_stage(JoinStage::Opening);
    downloaded.write(WorldDownloaded(Arc::new(world)));
    Ok(())
}

fn player_avatar_frame(player: &PlayerSession, remotes: &mut RemoteAvatars, frame: AvatarFrame, now: f64) {
    if Some(frame.peer) == player.peer || !frame.is_finite() {
        return;
    }
    remotes.accept_frame(frame, now);
}

fn player_timeouts(mut commands: Commands, time: Res<Time>, player: Res<PlayerSession>, mut notices: MessageWriter<NetNotice>) {
    let now = time.elapsed_secs_f64();
    let Some(since) = player.stage_since else { return };
    if player.stage == JoinStage::AwaitingWelcome && now - since > WELCOME_TIMEOUT {
        let reason = format!("the host did not answer within {WELCOME_TIMEOUT} seconds");
        error!("net: {reason}");
        notices.write(NetNotice::JoinFailed { reason });
        end_session(&mut commands);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shell requests (both roles)
// ─────────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn handle_shell_requests(
    mut commands: Commands,
    link: Res<NetLink>,
    host: Option<ResMut<HostSession>>,
    player: Option<ResMut<PlayerSession>>,
    mut chats: MessageReader<SendChat>,
    mut ready: MessageReader<LocalWorldReady>,
    mut ends: MessageReader<EndSession>,
    mut notices: MessageWriter<NetNotice>,
    (mut prompts, mut closes, mut receipts): (
        MessageReader<PromptPurchase>,
        MessageReader<ClosePurchase>,
        MessageReader<SendReceipts>,
    ),
) {
    if let Some(end) = ends.read().last() {
        if let Some(host) = &host {
            for (conn, _) in host.peers.iter() {
                link.send(*conn, &ToPlayer::Kicked { reason: end.reason.clone() });
                link.close(*conn, &end.reason);
            }
            notices.write(NetNotice::HostEnded { reason: end.reason.clone() });
        } else {
            link.send(HOST_CONN, &ToHost::Goodbye);
            notices.write(NetNotice::Disconnected { reason: end.reason.clone() });
        }
        end_session(&mut commands);
        return;
    }

    for chat in chats.read() {
        let Some(text) = sanitize_chat(&chat.text) else { continue };
        if let Some(host) = &host {
            let name = host.config.host_name.clone();
            for (conn, _) in host.ready() {
                link.send(conn, &ToPlayer::Chat { peer: HOST_PEER, name: name.clone(), text: text.clone() });
            }
            notices.write(NetNotice::Chat { peer: HOST_PEER, name, text });
        } else {
            link.send(HOST_CONN, &ToHost::Chat { text });
        }
    }

    // Purchases. A prompt goes to a player in the world with room for it;
    // any other is answered "not purchased" at once, so the asking script
    // never waits on a prompt nobody saw.
    let mut host = host;
    for ask in prompts.read() {
        let Some(host) = host.as_mut() else { continue };
        let target = host
            .peers
            .iter_mut()
            .find(|(_, p)| p.peer == ask.peer && p.stage == PeerStage::Ready && p.prompts.len() < MAX_OPEN_PROMPTS);
        match target {
            Some((conn, p)) => {
                p.prompts.insert(ask.prompt);
                link.send(*conn, &ToPlayer::PurchasePrompt { prompt: ask.prompt, product: ask.product, expects: ask.expects });
            }
            None => {
                notices.write(NetNotice::PurchaseClosed { peer: ask.peer, prompt: ask.prompt, purchased: false });
            }
        }
    }
    if host.is_none() {
        for close in closes.read() {
            link.send(HOST_CONN, &ToHost::PurchaseClosed { prompt: close.prompt, purchased: close.purchased });
        }
        for list in receipts.read() {
            let purchase_ids = sanitize_receipts(&list.purchase_ids);
            if !purchase_ids.is_empty() {
                link.send(HOST_CONN, &ToHost::Receipts { purchase_ids });
            }
        }
    }

    if ready.read().last().is_some() {
        if let Some(mut player) = player {
            if player.stage == JoinStage::Opening {
                link.send(HOST_CONN, &ToHost::WorldReady);
                player.set_stage(JoinStage::Playing);
                let peer = player.peer.unwrap_or_default();
                let host_name = player.host_name.clone();
                info!("net: in the world as peer {peer}");
                notices.write(NetNotice::Joined { peer, host_name });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Avatar systems (both roles)
// ─────────────────────────────────────────────────────────────────────────────

/// A peer's avatar descriptor, as this machine uses it: within
/// [`MAX_APPEARANCE_BYTES`], valid, and carrying no motion. How fast a player
/// walks, runs, sprints and jumps is the Space's to say (its
/// `SpaceCharacterPolicy`, and its scripts), never the peer's, so every motion
/// override is cleared here, the one gate each peer's descriptor passes on the
/// host and on every Player.
///
/// A peer also never picks the host's own models or floods the avatar
/// runtime: a rig that names any `space://` asset (the Worker issues only
/// `bundled://characters/` rigs, and a `space://` one would wear any model in
/// the host's Space), or carries more than [`MAX_BONE_ALIASES`] aliases or a
/// name longer than [`MAX_BONE_NAME`], is dropped for the built-in rig of its
/// body; a descriptor with more than [`MAX_SLOTS`] items, two of one kind, or
/// an item id that is not 1 to 64 letters, digits, `_`, `.` or `-` is
/// refused.
fn parse_appearance(json: &str) -> Option<AvatarDescriptor> {
    if json.len() > MAX_APPEARANCE_BYTES {
        return None;
    }
    let mut desc: AvatarDescriptor = serde_json::from_str(json).ok()?;
    desc.validate().ok()?;
    desc.motion = MotionOverrides::default();
    if desc.slots.len() > MAX_SLOTS {
        return None;
    }
    let mut kinds = HashSet::new();
    for (kind, item) in &desc.slots {
        let plain = |id: &str| {
            (1..=64).contains(&id.len()) && id.bytes().all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
        };
        if !kinds.insert(*kind) || item.as_ref().is_some_and(|ItemId(id)| !plain(id)) {
            return None;
        }
    }
    let foreign_rig = desc.rig.as_ref().is_some_and(|rig| {
        std::iter::once(&rig.body_asset).chain(rig.animations.iter()).any(|asset| !asset.starts_with("bundled://"))
            || rig.bone_aliases.len() > MAX_BONE_ALIASES
            || rig.bone_aliases.iter().any(|(from, to)| from.len() > MAX_BONE_NAME || to.len() > MAX_BONE_NAME)
    });
    if foreign_rig {
        desc.rig = None;
    }
    Some(desc)
}

/// A peer's appearance as the host keeps and relays it: the descriptor
/// [`parse_appearance`] cleaned, written out again, so nobody receives the
/// peer's own text.
fn clean_appearance(json: &str) -> Option<(AvatarDescriptor, String)> {
    let desc = parse_appearance(json)?;
    let clean = serde_json::to_string(&desc).ok().filter(|c| c.len() <= MAX_APPEARANCE_BYTES)?;
    Some((desc, clean))
}

/// Tell everyone what the local avatar looks like, once per avatar.
fn announce_local_appearance(
    link: Res<NetLink>,
    host: Option<ResMut<HostSession>>,
    player: Option<Res<PlayerSession>>,
    mut local: ResMut<LocalAvatarNet>,
    avatar: Query<(Entity, &AvatarDescriptor), (With<LocalAvatar>, With<SpawnedByAvatarRuntime>)>,
) {
    let Ok((entity, descriptor)) = avatar.single() else { return };
    if local.announced == Some(entity) {
        return;
    }
    if let Some(player) = &player {
        // The host ignores appearances before Hello; wait for the Welcome.
        if player.peer.is_none() {
            return;
        }
    }
    let Ok(json) = serde_json::to_string(descriptor) else { return };
    local.announced = Some(entity);
    if json.len() > MAX_APPEARANCE_BYTES {
        warn!("net: the local avatar descriptor is too large to share ({} bytes)", json.len());
        return;
    }
    if let Some(mut host) = host {
        for (conn, _) in host.ready() {
            link.send(conn, &ToPlayer::Appearance { peer: HOST_PEER, descriptor_json: json.clone() });
        }
        host.appearance = Some(json);
    } else {
        link.send(HOST_CONN, &ToHost::Appearance { descriptor_json: json });
    }
}

/// Record the local avatar's intent between input and locomotion, where the
/// jump edge is still visible (locomotion consumes it).
fn capture_local_intent(
    mut local: ResMut<LocalAvatarNet>,
    avatar: Query<&AvatarIntent, (With<LocalAvatar>, With<SpawnedByAvatarRuntime>)>,
) {
    let Ok(intent) = avatar.single() else { return };
    local.direction = intent.direction;
    local.sprint = intent.sprint;
    local.crouch = intent.crouch;
    if intent.jump_pressed {
        local.jumps = local.jumps.wrapping_add(1);
    }
}

/// Move the local avatar to where the host has it, after the host refused
/// its movement ([`ToPlayer::Correct`]), so it walks on from there. Not while
/// it rides a seat (the seat places it), and not within [`TELEPORT_HOLD`] of
/// something on this machine moving it farther than a step (the host's
/// teleport, replicated): a correction sent before that would undo it.
fn apply_host_correction(
    time: Res<Time>,
    mut local: ResMut<LocalAvatarNet>,
    mut avatar: Query<(&mut Transform, Has<AvatarSeated>), (With<LocalAvatar>, With<SpawnedByAvatarRuntime>)>,
) {
    let Ok((mut tf, seated)) = avatar.single_mut() else {
        local.correction = None;
        local.placed = None;
        return;
    };
    if let Some(to) = local.take_correction(tf.translation, seated, time.elapsed_secs_f64()) {
        tf.translation = to;
    }
}

impl LocalAvatarNet {
    /// Where to move the local avatar, now at `current`, this frame: the
    /// host's correction, unless it is `seated` or something on this machine
    /// moved it farther than a step within [`TELEPORT_HOLD`]. Notes where it
    /// ends the frame.
    fn take_correction(&mut self, current: Vec3, seated: bool, now: f64) -> Option<Vec3> {
        let correction = self.correction.take();
        if self.placed.is_some_and(|p| current.distance(p) > SNAP_DISTANCE) {
            self.moved_at = Some(now);
        }
        let settled = self.moved_at.is_none_or(|at| now - at >= TELEPORT_HOLD);
        let to = correction.filter(|_| !seated && settled);
        self.placed = Some(to.unwrap_or(current));
        to
    }
}

/// A host's correction as a place in the world, or nothing: a broken or
/// hostile host never writes a NaN, or a place past [`WORLD_LIMIT`], into
/// the local avatar.
fn usable_correction(position: [f32; 3]) -> Option<Vec3> {
    let to = Vec3::from_array(position);
    (to.is_finite() && to.abs().max_element() < WORLD_LIMIT).then_some(to)
}

/// Send the local avatar's state at [`AVATAR_SEND_HZ`].
fn send_local_avatar(
    time: Res<Time>,
    link: Res<NetLink>,
    host: Option<Res<HostSession>>,
    player: Option<Res<PlayerSession>>,
    mut local: ResMut<LocalAvatarNet>,
    avatar: Query<(&Transform, Option<&AvatarLocomotion>), (With<LocalAvatar>, With<SpawnedByAvatarRuntime>)>,
) {
    let period = 1.0 / AVATAR_SEND_HZ;
    local.accum += time.delta_secs();
    if local.accum < period {
        return;
    }
    // Never build a debt: a long frame sends one sample, not a burst.
    local.accum = (local.accum - period).min(period);

    let Ok((tf, loco)) = avatar.single() else { return };
    local.seq = local.seq.wrapping_add(1);
    let (yaw, _, _) = tf.rotation.to_euler(EulerRot::YXZ);
    let mut flags = 0;
    if local.sprint {
        flags |= AvatarFrame::SPRINT;
    }
    if local.crouch {
        flags |= AvatarFrame::CROUCH;
    }
    if loco.map(|l| l.grounded).unwrap_or(true) {
        flags |= AvatarFrame::GROUNDED;
    }
    let mut frame = AvatarFrame {
        peer: HOST_PEER,
        seq: local.seq,
        position: tf.translation.to_array(),
        yaw,
        direction: local.direction.to_array(),
        vertical_velocity: loco.map(|l| l.vertical_velocity).unwrap_or(0.0),
        flags,
        jumps: local.jumps,
    };
    if !frame.is_finite() {
        return;
    }

    if let Some(host) = &host {
        let bytes = encode_datagram(&Datagram::Avatar(frame));
        for (conn, p) in host.ready() {
            if p.datagrams {
                link.datagram(conn, bytes.clone());
            } else {
                link.send(conn, &ToPlayer::Avatar(frame));
            }
        }
    } else if let Some(player) = &player {
        if player.stage != JoinStage::Playing {
            return;
        }
        frame.peer = player.peer.unwrap_or_default();
        if player.datagrams {
            link.datagram(HOST_CONN, encode_datagram(&Datagram::Avatar(frame)));
        } else {
            link.send(HOST_CONN, &ToHost::Avatar(frame));
        }
    }
}

/// Request a replica for the next participant whose body and position are
/// both known, or a new body for one whose look changed (at most
/// [`REBUILDS_PER_SECOND`] across everyone, after every first spawn). One at
/// a time, so each new avatar can be matched to its peer.
fn spawn_replicas(
    time: Res<Time>,
    mut remotes: ResMut<RemoteAvatars>,
    policy: Option<Res<SpaceCharacterPolicy>>,
    mut spawn: MessageWriter<SpawnAvatar>,
) {
    let now = time.elapsed_secs_f64();
    if let Some(flight) = &remotes.in_flight {
        if now - flight.since < 2.0 {
            return;
        }
        // The runtime never produced it (for instance, it rejected the
        // descriptor). A first spawn is retried on a later frame; a rebuild
        // keeps the old body, in the look it was built with, and drops the
        // new look rather than retrying it.
        let peer = remotes.in_flight.take().map(|flight| flight.peer);
        if let Some(r) = peer.and_then(|peer| remotes.peers.get_mut(&peer)) {
            if r.entity.is_some() {
                if let Some(old) = r.previous_look.take() {
                    r.appearance = Some(old);
                }
                r.respawn = false;
            }
        }
    }
    while remotes.rebuilds.front().is_some_and(|t| now - t >= 1.0) {
        remotes.rebuilds.pop_front();
    }
    let rebuild_allowed = remotes.rebuilds.len() < REBUILDS_PER_SECOND;
    let ready = |r: &Remote| r.appearance.is_some() && r.latest.is_some();
    let next = remotes
        .peers
        .iter()
        .find(|(_, r)| ready(r) && r.entity.is_none())
        .or_else(|| remotes.peers.iter().find(|(_, r)| rebuild_allowed && ready(r) && r.respawn))
        .and_then(|(peer, r)| Some((*peer, r.appearance.clone()?, r.latest?, r.entity.is_some())));
    if let Some((peer, look, frame, rebuild)) = next {
        // Dressed by the open Space, as the local avatar is: its character for
        // this body option, and its motion.
        let desc = match policy.as_deref() {
            Some(policy) => policy.apply(look.clone()),
            None => look.clone(),
        };
        // A frame carries the capsule centre; spawn takes the feet. Starting a
        // metre low is corrected on the first frame after spawn.
        let at = Vec3::from_array(frame.position) - Vec3::Y;
        spawn.write(SpawnAvatar::new(desc, at).with_yaw(frame.yaw).with_control(AvatarControl::Remote));
        remotes.in_flight = Some(InFlight { peer, at, since: now, look });
        if rebuild {
            remotes.rebuilds.push_back(now);
            if let Some(r) = remotes.peers.get_mut(&peer) {
                r.respawn = false;
            }
        }
    }
}

/// Claim the avatar the runtime just built for the peer in flight. A new
/// body for a changed look replaces the old one in the same frame, so the
/// host never goes a frame without a collider for that player.
fn tag_replicas(
    mut commands: Commands,
    mut remotes: ResMut<RemoteAvatars>,
    fresh: Query<
        (Entity, &AvatarBody, &Transform),
        (Added<SpawnedByAvatarRuntime>, Without<LocalAvatar>, Without<NetReplica>),
    >,
) {
    let Some(at) = remotes.in_flight.as_ref().map(|flight| flight.at) else { return };
    for (entity, body, tf) in &fresh {
        if body.control != AvatarControl::Remote || tf.translation.distance(at) > 6.0 {
            continue;
        }
        match remotes.claim(entity) {
            Some(Claimed::Replica { peer, name, replaced }) => {
                if let Some(old) = replaced {
                    commands.entity(old).try_despawn();
                }
                commands.entity(entity).insert((NetReplica { peer }, Name::new(format!("Player: {name}"))));
            }
            Some(Claimed::Orphan) => commands.entity(entity).try_despawn(),
            None => {}
        }
        break;
    }
}

/// What became of an avatar the runtime built for the replica in flight.
#[derive(Debug, PartialEq)]
enum Claimed {
    /// It is now `peer`'s replica, replacing the body given (a rebuild's).
    Replica { peer: PeerId, name: String, replaced: Option<Entity> },
    /// The peer left while it was being built.
    Orphan,
}

impl RemoteAvatars {
    /// The avatar runtime built `entity` for the replica in flight.
    fn claim(&mut self, entity: Entity) -> Option<Claimed> {
        let flight = self.in_flight.take()?;
        let Some(r) = self.peers.get_mut(&flight.peer) else { return Some(Claimed::Orphan) };
        let replaced = r.entity.replace(entity);
        // Another change waiting falls back to this body's look.
        r.previous_look = if r.respawn { Some(flight.look) } else { None };
        Some(Claimed::Replica { peer: flight.peer, name: r.name.clone(), replaced })
    }
}

/// Walk each replica with its sender's intent.
fn drive_replica_intent(
    time: Res<Time>,
    mut remotes: ResMut<RemoteAvatars>,
    mut replicas: Query<(&NetReplica, &mut AvatarIntent)>,
) {
    let now = time.elapsed_secs_f64();
    for (replica, mut intent) in &mut replicas {
        let Some(r) = remotes.peers.get_mut(&replica.peer) else { continue };
        let Some(frame) = r.latest else { continue };
        if now - r.received_at > STALE_AFTER {
            intent.direction = Vec3::ZERO;
            intent.sprint = false;
            continue;
        }
        let dir = Vec3::from_array(frame.direction);
        intent.direction = if dir.length_squared() > 1.0 { dir.normalize_or_zero() } else { dir };
        intent.sprint = frame.has(AvatarFrame::SPRINT);
        intent.crouch = frame.has(AvatarFrame::CROUCH);
        match r.applied_jumps {
            // The first sample only sets the baseline; it is not a press.
            None => r.applied_jumps = Some(frame.jumps),
            Some(j) if j != frame.jumps => {
                intent.jump_pressed = true;
                r.applied_jumps = Some(frame.jumps);
            }
            Some(_) => {}
        }
    }
}

/// Pull each replica toward where its sender says it is. A seated replica
/// rides its seat instead (`AvatarSeated`), as its sender's avatar does.
fn correct_replicas(
    time: Res<Time>,
    mut remotes: ResMut<RemoteAvatars>,
    mut replicas: Query<(&NetReplica, &mut Transform), Without<AvatarSeated>>,
    mut reset: Local<HashSet<PeerId>>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs_f64();
    let move_blend = 1.0 - (-dt * 10.0).exp();
    let turn_blend = 1.0 - (-dt * 12.0).exp();
    for (replica, mut tf) in &mut replicas {
        let Some(r) = remotes.peers.get_mut(&replica.peer) else { continue };
        let Some(frame) = r.latest else { continue };
        let sample = Vec3::from_array(frame.position);
        let facing = Quat::from_rotation_y(frame.yaw);
        // The sender is the truth for its own avatar. A pose the local avatar
        // runtime made non-finite starts again from the sample, before the
        // bones and foot probes read it.
        if !tf.translation.is_finite() || !tf.rotation.is_finite() {
            if reset.insert(replica.peer) {
                warn!("net: peer {}'s avatar went non-finite on this machine; placed at its last sample", replica.peer);
            }
            tf.translation = sample;
            tf.rotation = facing;
            r.placed = Some(sample);
            r.teleport_hold = None;
            continue;
        }
        if let Some(target) = r.correction_target(tf.translation, sample, now) {
            let error = target - tf.translation;
            if error.length() > SNAP_DISTANCE {
                tf.translation = target;
            } else {
                tf.translation += error * move_blend;
            }
            tf.rotation = tf.rotation.slerp(facing, turn_blend);
        }
        r.placed = Some(tf.translation);
    }
}

/// When a session ends, its replicas go with it.
fn clear_replicas_after_session(
    mut commands: Commands,
    mut remotes: ResMut<RemoteAvatars>,
    mut local: ResMut<LocalAvatarNet>,
    mut inputs: ResMut<PeerInputs>,
    mut paces: ResMut<PeerPaces>,
    replicas: Query<Entity, With<NetReplica>>,
) {
    for e in &replicas {
        commands.entity(e).try_despawn();
    }
    if !paces.0.is_empty() {
        paces.0.clear();
    }
    if !inputs.by_peer.is_empty() {
        inputs.by_peer.clear();
    }
    if !remotes.peers.is_empty() || remotes.in_flight.is_some() {
        remotes.peers.clear();
        remotes.in_flight = None;
    }
    if local.announced.is_some() {
        local.announced = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(peer: PeerId, seq: u32) -> AvatarFrame {
        AvatarFrame {
            peer,
            seq,
            position: [0.0; 3],
            yaw: 0.0,
            direction: [0.0; 3],
            vertical_velocity: 0.0,
            flags: 0,
            jumps: 0,
        }
    }

    #[test]
    fn older_and_duplicate_samples_are_dropped_across_wraparound() {
        let mut r = RemoteAvatars::default();
        r.add_peer(1, "A".into());
        r.accept_frame(frame(1, u32::MAX), 1.0);
        r.accept_frame(frame(1, 0), 2.0); // wrapped: newer
        assert_eq!(r.peers[&1].latest.unwrap().seq, 0);
        r.accept_frame(frame(1, u32::MAX - 5), 3.0); // older
        assert_eq!(r.peers[&1].latest.unwrap().seq, 0);
        r.accept_frame(frame(1, 0), 4.0); // duplicate
        assert_eq!(r.peers[&1].received_at, 2.0);
    }

    #[test]
    fn frames_for_unknown_peers_are_ignored() {
        let mut r = RemoteAvatars::default();
        r.accept_frame(frame(9, 1), 1.0);
        assert!(r.peers.is_empty());
    }

    #[test]
    fn a_host_teleport_holds_until_the_sender_catches_up() {
        let mut r = RemoteAvatars::default();
        r.add_peer(1, "A".into());
        let p = r.peers.get_mut(&1).unwrap();
        let home = Vec3::ZERO;
        let far = Vec3::new(100.0, 0.0, 0.0);
        assert_eq!(p.correction_target(home, home, 0.0), Some(home));
        p.placed = Some(home);
        // A host script moves the replica 100 m; its sender still reports home.
        assert_eq!(p.correction_target(far, home, 0.1), None);
        p.placed = Some(far);
        assert_eq!(p.correction_target(far, home, 0.5), None);
        // The sender's own avatar arrives there: correction resumes.
        assert_eq!(p.correction_target(far, far + Vec3::X, 0.6), Some(far + Vec3::X));
        // A move the sender never follows is given up after the hold.
        let elsewhere = Vec3::new(0.0, 0.0, 300.0);
        assert_eq!(p.correction_target(elsewhere, far, 1.0), None);
        assert_eq!(p.correction_target(elsewhere, far, 1.0 + TELEPORT_HOLD + 0.01), Some(far));
    }

    #[test]
    fn only_usable_samples_are_kept() {
        let mut r = RemoteAvatars::default();
        r.add_peer(1, "A".into());
        let mut lost = frame(1, 1);
        lost.direction = [f32::NAN, 0.0, 0.0];
        let mut spun = frame(1, 2);
        spun.yaw = f32::INFINITY;
        let mut far = frame(1, 3);
        far.position = [0.0, -WORLD_LIMIT, 0.0];
        for (at, f) in [lost, spun, far].into_iter().enumerate() {
            r.accept_frame(f, at as f64);
        }
        assert!(r.peers[&1].latest.is_none(), "a replica never spawns from them");
        r.accept_frame(frame(1, 4), 4.0);
        assert_eq!(r.peers[&1].latest.map(|f| f.seq), Some(4));
    }

    #[test]
    fn spawn_points_spread_and_names_stay_unique() {
        let mut host = HostSession::new(host_config());
        let a = host.spawn_for(1);
        let b = host.spawn_for(2);
        assert!(Vec3::from_array(a).distance(Vec3::from_array(b)) > 1.0);
        assert_eq!(host.unique_name("Host", 1), "Host (2)");
        assert_eq!(host.unique_name("", 3), "Player3");
        let mut ada = HostPeer::new(1, String::new(), true, 0.0);
        ada.name = "Ada".into();
        ada.stage = PeerStage::Ready;
        host.peers.insert(7, ada);
        assert_eq!(host.unique_name("Ada", 2), "Ada (2)");
    }

    fn host_config() -> HostConfig {
        HostConfig {
            host_name: "Host".into(),
            max_players: 8,
            world: WorldManifest::new("U", "0", 256.0),
            chunks: HashMap::new(),
            spawn: [0.0, 1.0, 0.0],
            sim_id: None,
        }
    }

    #[test]
    fn live_world_frames_wait_for_the_catch_up() {
        let (link, ends) = link_pair();
        let mut host = HostSession::new(host_config());
        let mut p = HostPeer::new(4, String::new(), true, 0.0);
        p.stage = PeerStage::Ready;
        host.peers.insert(9, p);
        let frame = |tick| WorldFrame { tick, ops: vec![] };
        let send = |peer, ticks: &[u64], catch_up| SendWorld { peer, frames: ticks.iter().map(|t| frame(*t)).collect(), catch_up };
        assert_eq!(deliver_world(&link, &mut host, &send(4, &[1], false)), 0, "held: the catch-up includes it");
        assert_eq!(deliver_world(&link, &mut host, &send(4, &[2, 3], true)), 2);
        assert_eq!(deliver_world(&link, &mut host, &send(4, &[4], false)), 1);
        assert_eq!(deliver_world(&link, &mut host, &send(5, &[5], true)), 0, "no such player");
        let sent: Vec<u64> = ends
            .commands
            .try_iter()
            .filter_map(|c| match c {
                LinkCommand::Frame { conn: 9, frame } => match decode_body::<ToPlayer>(&frame[4..]) {
                    Ok(ToPlayer::World(w)) => Some(w.tick),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(sent, vec![2, 3, 4]);
    }

    #[test]
    fn tickets_are_kept_only_when_printable_and_short() {
        let mut host = HostSession::new(host_config());
        host.set_identity(Some("eus_t1.abc-DEF_09".into()));
        assert_eq!(host.identity.as_deref(), Some("eus_t1.abc-DEF_09"));
        for bad in ["", "has space", "line\nbreak", &"x".repeat(crate::wire::MAX_TICKET_CHARS + 1)] {
            host.set_identity(Some(bad.to_string()));
            assert_eq!(host.identity, None, "kept {bad:?}");
        }
        let player = PlayerSession::new("P", Some("tab\there".into()));
        assert_eq!(player.identity, None);
    }

    /// A player's ticket goes into its one Hello and nowhere else: the
    /// session keeps no copy, and a Hello's Debug never prints it.
    #[test]
    fn the_ticket_goes_into_one_hello_and_is_never_printed() {
        let secret = "eit1.c2VjcmV0LXRpY2tldA.c2ln-_x";
        let mut player = PlayerSession::new("Ada", Some(secret.into()));
        let first = player.hello("test".into());
        assert_eq!(first.identity.as_deref(), Some(secret));
        assert_eq!(player.identity, None, "the session kept a copy");
        assert_eq!(player.hello("test".into()).identity, None, "a second Hello carried it again");
        let shown = format!("{:?}", ToHost::Hello(first));
        assert!(!shown.contains(secret), "a Hello's Debug printed its ticket: {shown}");
        assert!(shown.contains("identity: <set>"), "{shown}");
    }

    /// A joiner's ticket in the host's hands is never printed, and no copy
    /// outlives the join: when the player enters the world the host takes it
    /// out of the peer and hands it, once, to the shell that checks it.
    #[test]
    fn the_host_never_prints_a_joiners_ticket_and_keeps_no_copy() {
        let secret = "eit1.c2VjcmV0LXRpY2tldA.c2ln-_x";
        let (mut app, ends) = admission_host(false, &[3]);
        say(&ends, 3, &hello(Some(secret)));
        app.update();
        {
            let held = &app.world().resource::<HostSession>().peers[&3].identity;
            assert_eq!(held.as_ref().map(Ticket::secret), Some(secret));
            assert!(!format!("{held:?}").contains(secret), "the peer's ticket printed");
        }
        say(&ends, 3, &ToHost::WorldReady);
        app.update();
        assert_eq!(app.world().resource::<HostSession>().peers[&3].identity, None, "a copy outlived the join");
        let notices: Vec<NetNotice> = app.world_mut().resource_mut::<Messages<NetNotice>>().drain().collect();
        let checked = notices.iter().find_map(|n| match n {
            NetNotice::PeerIdentity { ticket, .. } => Some(ticket.secret()),
            _ => None,
        });
        assert_eq!(checked, Some(secret), "the shell gets it once, to check it");
        for n in &notices {
            assert!(!format!("{n:?}").contains(secret), "a notice printed the ticket: {n:?}");
        }
        let admit = NetNotice::Admit { peer: 1, ticket: Some(Ticket(secret.into())) };
        assert!(!format!("{admit:?}").contains(secret), "{admit:?}");
    }

    /// A host app with the real pump and admission systems, the transport's
    /// end of its link, and one connected player per conn given.
    fn admission_host(admission: bool, conns: &[ConnId]) -> (App, LinkEnds) {
        let (link, ends) = link_pair();
        let mut host = HostSession::new(host_config());
        if admission {
            host.require_admission();
        }
        for (i, conn) in conns.iter().enumerate() {
            host.peers.insert(*conn, HostPeer::new(i as PeerId + 1, String::new(), true, 0.0));
        }
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<RemoteAvatars>()
            .init_resource::<PeerPaces>()
            .init_resource::<PeerInputs>()
            .add_message::<NetNotice>()
            .add_message::<RemoteArrived>()
            .add_message::<TracksArrived>()
            .add_message::<AdmissionVerdict>()
            .insert_resource(link)
            .insert_resource(host)
            .add_systems(Update, (host_pump, host_admissions).chain());
        (app, ends)
    }

    fn say(ends: &LinkEnds, conn: ConnId, msg: &ToHost) {
        let frame = encode_frame(msg).unwrap();
        ends.events.send(LinkEvent::Frame { conn, body: frame[4..].to_vec() }).unwrap();
    }

    fn hello(ticket: Option<&str>) -> ToHost {
        ToHost::Hello(Hello {
            protocol: PROTOCOL_VERSION,
            name: "Ada".into(),
            engine_version: "test".into(),
            identity: ticket.map(str::to_string),
        })
    }

    /// What the host sent `conn` on its reliable stream, and whether it closed it.
    fn sent_to(ends: &LinkEnds, conn: ConnId) -> (Vec<ToPlayer>, bool) {
        let mut msgs = Vec::new();
        let mut closed = false;
        for c in ends.commands.try_iter() {
            match c {
                LinkCommand::Frame { conn: to, frame } if to == conn => msgs.push(decode_body::<ToPlayer>(&frame[4..]).unwrap()),
                LinkCommand::Close { conn: to, .. } if to == conn => closed = true,
                _ => {}
            }
        }
        (msgs, closed)
    }

    fn admits(app: &mut App) -> Vec<(PeerId, Option<String>)> {
        app.world_mut()
            .resource_mut::<Messages<NetNotice>>()
            .drain()
            .filter_map(|n| match n {
                NetNotice::Admit { peer, ticket } => Some((peer, ticket.map(|t| t.secret().to_string()))),
                _ => None,
            })
            .collect()
    }

    /// A session the gallery lists sends nothing of the world until the
    /// gallery admits the player, and then exactly the Welcome.
    #[test]
    fn a_listed_session_waits_for_the_gallery_before_the_welcome() {
        let (mut app, ends) = admission_host(true, &[7]);
        say(&ends, 7, &hello(Some("eit1.a.b")));
        app.update();
        assert_eq!(admits(&mut app), vec![(1, Some("eit1.a.b".to_string()))], "the shell is asked");
        let (msgs, closed) = sent_to(&ends, 7);
        assert!(msgs.is_empty() && !closed, "nothing of the world before admission: {msgs:?}");

        // Asking for the world early is a protocol breach.
        let (mut early, early_ends) = admission_host(true, &[8]);
        say(&early_ends, 8, &hello(None));
        say(&early_ends, 8, &ToHost::RequestChunks { hashes: vec![] });
        early.update();
        let (msgs, closed) = sent_to(&early_ends, 8);
        assert!(matches!(msgs.as_slice(), [ToPlayer::Kicked { .. }]) && closed, "{msgs:?}");

        let ticket_id = "0123456789abcdef0123456789abcdef".to_string();
        app.world_mut().write_message(AdmissionVerdict {
            peer: 1,
            outcome: Ok(Admitted { ticket_id: ticket_id.clone(), guest: false }),
        });
        app.update();
        let (msgs, closed) = sent_to(&ends, 7);
        assert!(matches!(msgs.as_slice(), [ToPlayer::Welcome(w)] if w.peer == 1) && !closed, "{msgs:?}");
        assert_eq!(app.world().resource::<HostSession>().admitted_ids(), vec![ticket_id], "for the heartbeat");
    }

    /// The gallery's refusal reaches the player in its own words; silence
    /// for longer than the timeout refuses too; neither leaves a trace.
    #[test]
    fn a_refused_or_unanswered_join_is_turned_away() {
        let (mut app, ends) = admission_host(true, &[7, 8]);
        say(&ends, 7, &hello(Some("eit1.used")));
        say(&ends, 8, &hello(None));
        app.update();
        let used = "This ticket was already used. Join again from the gallery for a new one.";
        app.world_mut().write_message(AdmissionVerdict { peer: 1, outcome: Err(used.to_string()) });
        app.update();
        let msgs: Vec<ToPlayer> = ends
            .commands
            .try_iter()
            .filter_map(|c| match c {
                LinkCommand::Frame { conn: 7, frame } => decode_body::<ToPlayer>(&frame[4..]).ok(),
                _ => None,
            })
            .collect();
        assert_eq!(msgs, vec![ToPlayer::Refused { reason: used.to_string() }]);
        assert!(!app.world().resource::<HostSession>().peers.contains_key(&7));

        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f64(ADMISSION_TIMEOUT + 1.0));
        app.update();
        let (msgs, closed) = sent_to(&ends, 8);
        assert_eq!(msgs, vec![ToPlayer::Refused { reason: ADMISSION_UNCONFIRMED.to_string() }]);
        assert!(closed && app.world().resource::<HostSession>().peers.is_empty());
    }

    /// A session whose heartbeat the gallery refused before ever listing it
    /// is open again: the next join is welcomed at once, not asked about.
    #[test]
    fn a_released_session_welcomes_at_once() {
        let (mut app, ends) = admission_host(true, &[7]);
        app.world_mut().resource_mut::<HostSession>().release_admission();
        assert!(!app.world().resource::<HostSession>().requires_admission());
        say(&ends, 7, &hello(None));
        app.update();
        assert!(admits(&mut app).is_empty(), "the gallery is not asked");
        let (msgs, _) = sent_to(&ends, 7);
        assert!(matches!(msgs.as_slice(), [ToPlayer::Welcome(_)]), "{msgs:?}");
    }

    /// Without the gallery, a join is welcomed at once, as before; and a
    /// guest's chat is dropped.
    #[test]
    fn an_open_session_welcomes_at_once_and_guests_do_not_chat() {
        let (mut app, ends) = admission_host(false, &[7]);
        say(&ends, 7, &hello(None));
        app.update();
        assert!(admits(&mut app).is_empty());
        let (msgs, _) = sent_to(&ends, 7);
        assert!(matches!(msgs.as_slice(), [ToPlayer::Welcome(_)]), "{msgs:?}");

        let mut host = HostSession::new(host_config());
        let mut p = HostPeer::new(4, String::new(), true, 0.0);
        p.stage = PeerStage::Ready;
        p.guest = true;
        host.peers.insert(9, p);
        let (link, ends) = link_pair();
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<RemoteAvatars>()
            .init_resource::<PeerPaces>()
            .init_resource::<PeerInputs>()
            .add_message::<NetNotice>()
            .add_message::<RemoteArrived>()
            .add_message::<TracksArrived>()
            .insert_resource(link)
            .insert_resource(host)
            .add_systems(Update, host_pump);
        say(&ends, 9, &ToHost::Chat { text: "hello".into() });
        app.update();
        let chats = app
            .world_mut()
            .resource_mut::<Messages<NetNotice>>()
            .drain()
            .filter(|n| matches!(n, NetNotice::Chat { .. }))
            .count();
        assert_eq!(chats, 0, "a guest's chat goes nowhere");
    }

    /// A descriptor asking to walk at 500 m/s, run at 900, sprint at ten
    /// times that, and jump 200 m high.
    fn greedy_descriptor() -> AvatarDescriptor {
        let mut desc = AvatarDescriptor::default();
        desc.motion.walk_speed_mps = Some(500.0);
        desc.motion.run_speed_mps = Some(900.0);
        desc.motion.sprint_multiplier = Some(10.0);
        desc.motion.jump_apex_m = Some(200.0);
        desc
    }

    /// Motion comes from the Space, never from a peer: every machine clears
    /// what a peer's descriptor asks for, so it moves as an honest one does.
    #[test]
    fn a_peer_never_chooses_its_own_motion() {
        use eustress_common::avatar::{resolve, NOMINAL_BIND_HEIGHT_M};
        let json = serde_json::to_string(&greedy_descriptor()).unwrap();
        let desc = parse_appearance(&json).expect("a valid descriptor is kept");
        assert_eq!(desc.motion, MotionOverrides::default());
        let honest = resolve(&AvatarDescriptor::default(), NOMINAL_BIND_HEIGHT_M).1;
        let peer = resolve(&desc, NOMINAL_BIND_HEIGHT_M).1;
        assert_eq!(peer.walk_speed, honest.walk_speed);
        assert_eq!(peer.run_speed, honest.run_speed);
        assert_eq!(peer.jump_velocity(), honest.jump_velocity());
        assert!(peer.walk_speed < 500.0);
        assert!(clean_appearance("{ not json").is_none());
        assert!(clean_appearance(&" ".repeat(MAX_APPEARANCE_BYTES + 1)).is_none());
    }

    /// The host keeps and relays the descriptor it cleaned, never the peer's
    /// own text.
    #[test]
    fn the_host_relays_the_appearance_it_cleaned() {
        let (mut app, ends) = admission_host(false, &[1, 2]);
        for p in app.world_mut().resource_mut::<HostSession>().peers.values_mut() {
            p.stage = PeerStage::Ready;
        }
        say(&ends, 1, &ToHost::Appearance { descriptor_json: serde_json::to_string(&greedy_descriptor()).unwrap() });
        app.update();

        let relayed: Vec<String> = ends
            .commands
            .try_iter()
            .filter_map(|c| match c {
                LinkCommand::Frame { conn: 2, frame } => match decode_body::<ToPlayer>(&frame[4..]) {
                    Ok(ToPlayer::Appearance { peer: 1, descriptor_json }) => Some(descriptor_json),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(relayed.len(), 1, "the other player hears it once");
        let heard: AvatarDescriptor = serde_json::from_str(&relayed[0]).unwrap();
        assert_eq!(heard.motion, MotionOverrides::default(), "no motion leaves the host");
        let kept = app.world().resource::<HostSession>().peers[&1].appearance.clone();
        assert_eq!(kept.as_deref(), Some(relayed[0].as_str()), "late joiners get the same cleaned text");
    }

    /// A replica is dressed by the open Space, as the local avatar is, and
    /// moves as an honest avatar under the same Space would.
    #[test]
    fn replicas_are_dressed_by_the_space_and_move_at_its_speed() {
        use eustress_common::avatar::{resolve, RigDefinition, NOMINAL_BIND_HEIGHT_M};
        let mut rig = RigDefinition::builtin(AvatarDescriptor::default().resolved_identity());
        rig.id = "space-hero".into();
        let policy = SpaceCharacterPolicy { fallback: Some(rig), ..Default::default() };
        let mut remotes = RemoteAvatars::default();
        remotes.add_peer(3, "Cy".into());
        let json = serde_json::to_string(&greedy_descriptor()).unwrap();
        remotes.set_appearance(3, parse_appearance(&json).unwrap());
        remotes.accept_frame(frame(3, 1), 0.0);

        let mut app = App::new();
        app.init_resource::<Time>()
            .add_message::<SpawnAvatar>()
            .insert_resource(policy.clone())
            .insert_resource(remotes)
            .add_systems(Update, spawn_replicas);
        app.update();

        let spawned: Vec<SpawnAvatar> = app.world_mut().resource_mut::<Messages<SpawnAvatar>>().drain().collect();
        assert_eq!(spawned.len(), 1);
        let desc = spawned[0].descriptor();
        assert_eq!(desc.rig.as_ref().map(|r| r.id.as_str()), Some("space-hero"), "the Space's character");
        let honest = policy.apply(AvatarDescriptor::default());
        assert_eq!(desc.motion, honest.motion, "the Space's motion, nothing of the peer's");
        let speed = resolve(desc, NOMINAL_BIND_HEIGHT_M).1.walk_speed;
        assert_eq!(speed, resolve(&honest, NOMINAL_BIND_HEIGHT_M).1.walk_speed);
        assert!(speed < 500.0);
    }

    // ── Movement (Part B) ───────────────────────────────────────────────────

    use eustress_common::avatar::{ItemId, RigDefinition, SlotKind};
    use std::time::Duration;

    const MOVER: ConnId = 7;

    /// The pace of an avatar whose own motion runs it at most `top` m/s and
    /// jumps it off the ground at `rise` m/s, on nothing that moves.
    fn pace(top: f32, rise: f32) -> Pace {
        Pace { limit: PaceLimit::of(top, rise, 0.0), seated: false, place: Vec3::ZERO, carried_jump: None }
    }

    fn set_pace(paces: &mut PeerPaces, peer: PeerId, pace: Pace) {
        let track = paces.0.entry(peer).or_insert_with(|| PeerPace::new(pace.place));
        track.pace = pace;
        track.carried_jump = pace.carried_jump;
    }

    /// One player (peer 1) in the world, its budget started at the origin at
    /// time 0, measured at `pace` once its replica exists (`None`: not yet).
    struct Mover {
        link: NetLink,
        ends: LinkEnds,
        host: HostSession,
        remotes: RemoteAvatars,
        paces: PeerPaces,
        seq: u32,
        /// The Space checks movement (StarterPlayer ServerMovementCheck).
        checked: bool,
    }

    impl Mover {
        fn new(pace: Option<Pace>) -> Self {
            let (link, ends) = link_pair();
            let mut host = HostSession::new(host_config());
            let mut p = HostPeer::new(1, String::new(), true, 0.0);
            p.stage = PeerStage::Ready;
            p.travel = Some(TravelBudget::start(0.0, Vec3::ZERO, pace.map_or_else(PaceLimit::before_replica, |p| p.limit)));
            host.peers.insert(MOVER, p);
            let mut remotes = RemoteAvatars::default();
            remotes.add_peer(1, "Ada".into());
            let mut paces = PeerPaces::default();
            if let Some(pace) = pace {
                set_pace(&mut paces, 1, pace);
            }
            Self { link, ends, host, remotes, paces, seq: 0, checked: true }
        }

        /// A sample at `pos`, `now` seconds in; true when the host kept it.
        fn sample(&mut self, pos: Vec3, now: f64) -> bool {
            self.seq += 1;
            let sample = AvatarFrame { position: pos.to_array(), ..frame(1, self.seq) };
            host_avatar_frame(&self.link, &mut self.host, &mut self.remotes, &mut self.paces, self.checked, MOVER, sample, now);
            self.remotes.peers[&1].latest.is_some_and(|f| f.seq == self.seq)
        }

        /// Whether the host removed the player, and where it said the player is.
        fn outcome(&self) -> (bool, Vec<Vec3>) {
            let (msgs, closed) = sent_to(&self.ends, MOVER);
            let corrections = msgs
                .iter()
                .filter_map(|m| match m {
                    ToPlayer::Correct { position } => Some(Vec3::from_array(*position)),
                    _ => None,
                })
                .collect();
            (closed, corrections)
        }
    }

    /// Twice its pace for ten seconds: refused once its banked second runs
    /// out, told where the host has it at most twice a second, and removed.
    #[test]
    fn a_sustained_run_past_the_pace_is_refused_until_the_player_is_removed() {
        let mut m = Mover::new(Some(pace(4.0, 0.0))); // 5.1 m/s with the margin
        let mut refused_from = None;
        for i in 1..=200 {
            let t = i as f64 / 20.0;
            if !m.sample(Vec3::X * 8.0 * t as f32, t) && refused_from.is_none() {
                refused_from = Some(t);
            }
        }
        let refused_from = refused_from.expect("never refused");
        assert!((1.5..2.2).contains(&refused_from), "first refused at {refused_from} s");
        let (removed, corrections) = m.outcome();
        assert!(removed, "a sustained run removes the player");
        let span = 10.0 - refused_from;
        assert!(!corrections.is_empty() && corrections.len() as f64 <= span / CORRECTION_INTERVAL + 1.0, "{}", corrections.len());
        let kept = m.remotes.peers[&1].latest.map(|f| Vec3::from_array(f.position)).unwrap();
        assert!(corrections.iter().all(|c| c.distance(kept) < 1e-4), "each correction is its last accepted place");
    }

    /// A host script raised its WalkSpeed: walking at that pace, samples
    /// arriving 20 ms early and late, is never refused.
    #[test]
    fn walking_at_a_pace_a_host_script_raised_is_never_refused() {
        let mut m = Mover::new(Some(pace(20.0, 0.0)));
        for i in 1..=400 {
            let t = i as f64 / 20.0;
            let arrives = t + if i % 2 == 0 { 0.02 } else { -0.02 };
            assert!(m.sample(Vec3::Z * 20.0 * t as f32, arrives), "refused at {t} s");
        }
        assert_eq!(m.outcome(), (false, vec![]));
    }

    /// Standing still banks one second of pace, no more: a burst within it
    /// is kept, and the same speed kept up is refused within half a second.
    #[test]
    fn a_burst_after_standing_still_is_kept_within_one_second_of_pace() {
        let mut m = Mover::new(Some(pace(4.0, 0.0))); // 5.1 m/s
        for i in 1..=40 {
            assert!(m.sample(Vec3::ZERO, i as f64 / 20.0));
        }
        // 9 m/s for a second: 5.1 m earned and 3.9 of the 5.1 banked.
        for i in 1..=20 {
            let t = i as f64 / 20.0;
            assert!(m.sample(Vec3::X * 9.0 * t as f32, 2.0 + t), "refused {t} s into the burst");
        }
        let refused = (21..=30).filter(|&i| !m.sample(Vec3::X * 9.0 * i as f32 / 20.0, 2.0 + i as f64 / 20.0)).count();
        assert!(refused > 0, "kept up past the bank");
    }

    /// Lost packets: the time since the last kept sample counts, up to
    /// STALL_CAP_S, so a peer that withholds samples gains ten seconds of
    /// pace at most.
    #[test]
    fn a_stall_is_forgiven_up_to_its_cap() {
        let mut m = Mover::new(Some(pace(4.0, 0.0))); // 5.1 m/s
        assert!(m.sample(Vec3::X * 15.0, 3.0), "three seconds of walking after three silent ones");
        assert!(!m.sample(Vec3::X * 315.0, 63.0), "a minute's silence buys ten seconds, not sixty");
        assert!(m.sample(Vec3::X * 65.0, 63.05), "ten seconds of pace from the last kept sample");
    }

    /// A seat's vehicle moves its rider at any speed. Out of the seat, the
    /// rider is measured from where the host has it, not from the samples
    /// it sent while seated.
    #[test]
    fn a_seat_carries_a_player_anywhere_and_leaving_it_starts_from_the_seat() {
        let seat = Vec3::new(500.0, 0.0, 0.0);
        let mut m = Mover::new(Some(Pace { seated: true, place: seat, ..pace(4.0, 0.0) }));
        assert!(m.sample(Vec3::new(100.0, 0.0, 0.0), 0.05));
        assert!(m.sample(Vec3::new(400.0, 0.0, 0.0), 0.10));
        set_pace(&mut m.paces, 1, Pace { place: seat, ..pace(4.0, 0.0) });
        assert!(!m.sample(Vec3::new(400.0, 0.0, 0.0), 0.15), "its seated samples are not where it is");
        assert!(m.sample(seat + Vec3::X, 0.20));
    }

    /// A seated player's samples are kept and relayed at its seat, wherever
    /// they say it is: the other players see it there, its replica leaves
    /// the seat from there, and a new body for a changed look spawns there.
    #[test]
    fn a_seated_player_is_where_its_seat_is() {
        let seat = Vec3::new(40.0, 2.0, 0.0);
        let mut m = Mover::new(Some(Pace { seated: true, place: seat, ..pace(4.0, 0.0) }));
        let mut other = HostPeer::new(2, String::new(), true, 0.0);
        other.stage = PeerStage::Ready;
        m.host.peers.insert(8, other);
        assert!(m.sample(Vec3::new(540.0, 2.0, 0.0), 0.05), "kept, at its seat");
        assert_eq!(m.remotes.peers[&1].latest.map(|f| Vec3::from_array(f.position)), Some(seat));
        let relayed: Vec<Vec3> = m
            .ends
            .commands
            .try_iter()
            .filter_map(|c| match c {
                LinkCommand::Datagram { conn: 8, bytes } => match decode_datagram(&bytes) {
                    Ok(Datagram::Avatar(f)) => Some(Vec3::from_array(f.position)),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(relayed, vec![seat], "the others see it at its seat");

        let mut remotes = std::mem::take(&mut m.remotes);
        let old = World::new().spawn_empty().id();
        remotes.set_appearance(1, in_hat("cap"));
        remotes.peers.get_mut(&1).unwrap().entity = Some(old);
        remotes.set_appearance(1, in_hat("crown"));
        let mut app = App::new();
        app.init_resource::<Time>().add_message::<SpawnAvatar>().insert_resource(remotes).add_systems(Update, spawn_replicas);
        app.update();
        let spawned: Vec<SpawnAvatar> = app.world_mut().resource_mut::<Messages<SpawnAvatar>>().drain().collect();
        assert_eq!(spawned.len(), 1);
        assert_eq!(spawned[0].at(), seat - Vec3::Y, "the new body spawns at the seat");
    }

    /// One sample whose seq leaps ahead would make every later honest
    /// sample look older, freezing the host's replica while everyone else
    /// saw the player move: it is refused, and the samples after it are kept
    /// and relayed. A reordered older sample is dropped and not relayed, a
    /// gap a long loss explains is kept, and stale samples never move the
    /// kept one.
    #[test]
    fn a_seq_leap_is_refused_and_only_kept_samples_are_relayed() {
        let mut m = Mover::new(Some(pace(4.0, 0.0)));
        let mut other = HostPeer::new(2, String::new(), true, 0.0);
        other.stage = PeerStage::Ready;
        m.host.peers.insert(8, other);
        let send = |m: &mut Mover, seq: u32, x: f32, now: f64| {
            let sample = AvatarFrame { position: [x, 0.0, 0.0], ..frame(1, seq) };
            host_avatar_frame(&m.link, &mut m.host, &mut m.remotes, &mut m.paces, m.checked, MOVER, sample, now);
        };
        let kept = |m: &Mover| m.remotes.peers[&1].latest.map(|f| (f.seq, f.position[0]));
        send(&mut m, 10, 0.1, 0.05);
        send(&mut m, 10 + (1 << 31) - 1, 0.2, 0.10);
        assert_eq!(m.host.peers[&MOVER].violations, 1, "the leap is refused");
        send(&mut m, 11, 0.3, 0.15);
        assert_eq!(kept(&m), Some((11, 0.3)), "honest samples go on being kept");
        send(&mut m, 9, 0.0, 0.20);
        assert_eq!(kept(&m), Some((11, 0.3)), "a reordered one is dropped");
        send(&mut m, 11 + 3000, 1.0, 0.25);
        assert_eq!(kept(&m), Some((3011, 1.0)), "3000 lost samples");
        for i in 0..50 {
            send(&mut m, 3010 - i, 0.5, 0.3 + i as f64 * 0.01);
        }
        assert_eq!(kept(&m), Some((3011, 1.0)), "50 stale samples move nothing");
        assert_eq!(m.host.peers[&MOVER].violations, 1);
        let relayed: Vec<u32> = m
            .ends
            .commands
            .try_iter()
            .filter_map(|c| match c {
                LinkCommand::Datagram { conn: 8, bytes } => match decode_datagram(&bytes) {
                    Ok(Datagram::Avatar(f)) => Some(f.seq),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(relayed, vec![10, 11, 3011], "only what the host kept");
        assert_eq!(seq_order(5 + MAX_SEQ_GAP, Some(5)), SeqOrder::Newer);
        assert_eq!(seq_order(6 + MAX_SEQ_GAP, Some(5)), SeqOrder::Jump);
        assert_eq!(seq_order(2, Some(u32::MAX)), SeqOrder::Newer, "across the wrap");
        assert_eq!(seq_order(7, None), SeqOrder::Newer, "a connection's first");
    }

    /// A Space that turns the movement check off keeps its players'
    /// movement as sent; being finite and inside the world still holds, and
    /// turned on again, the check measures from where it last kept the
    /// player.
    #[test]
    fn a_space_can_turn_the_movement_check_off() {
        let mut m = Mover::new(Some(pace(4.0, 0.0)));
        m.checked = false;
        assert!(m.sample(Vec3::X * 500.0, 0.05), "a leap the check would refuse");
        assert_eq!(m.outcome(), (false, vec![]), "no correction, nothing counted");
        assert!(!m.sample(Vec3::new(f32::NAN, 0.0, 0.0), 0.10));
        assert!(!m.sample(Vec3::X * 2.0e5, 0.15), "still inside the world");
        m.checked = true;
        assert!(!m.sample(Vec3::X * 1000.0, 0.20));
        assert!(m.sample(Vec3::X * 501.0, 0.25), "measured from the last kept place");
    }

    /// The switch comes from the host's own Space (its SpaceCharacterPolicy
    /// resource), never from a player: unset, the check is on.
    #[test]
    fn the_host_reads_the_movement_check_from_its_own_space() {
        for (setting, kept) in [(None, false), (Some(true), false), (Some(false), true)] {
            let (mut app, ends) = admission_host(false, &[3]);
            app.insert_resource(SpaceCharacterPolicy { server_movement_check: setting, ..Default::default() });
            {
                let mut host = app.world_mut().resource_mut::<HostSession>();
                let p = host.peers.get_mut(&3).unwrap();
                p.stage = PeerStage::Ready;
                p.travel = Some(TravelBudget::start(0.0, Vec3::ZERO, PaceLimit::before_replica()));
            }
            app.world_mut().resource_mut::<RemoteAvatars>().add_peer(1, String::new());
            let sample = AvatarFrame { position: [500.0, 0.0, 0.0], ..frame(1, 1) };
            ends.events.send(LinkEvent::Datagram { conn: 3, bytes: encode_datagram(&Datagram::Avatar(sample)) }).unwrap();
            app.update();
            let latest = app.world().resource::<RemoteAvatars>().peers[&1].latest;
            assert_eq!(latest.is_some(), kept, "ServerMovementCheck {setting:?}");
        }
    }

    /// A flood of samples costs nothing: past MAX_SAMPLES_PER_SECOND they
    /// are dropped unread, uncounted and unrelayed.
    #[test]
    fn samples_past_the_rate_are_dropped_unread() {
        let mut m = Mover::new(Some(pace(4.0, 0.0)));
        let kept = (0..1000).filter(|_| m.sample(Vec3::ZERO, 0.5)).count();
        assert_eq!(kept, MAX_SAMPLES_PER_SECOND as usize, "a second's worth banked");
        let kept = (0..1000).filter(|_| m.sample(Vec3::ZERO, 1.0)).count();
        assert_eq!(kept, (MAX_SAMPLES_PER_SECOND / 2.0) as usize, "then the rate");
        assert_eq!(m.host.peers[&MOVER].violations, 0);
        assert_eq!(m.outcome(), (false, vec![]));
    }

    /// A host script's teleport moves the player once, to where it went.
    #[test]
    fn a_host_teleport_starts_the_budget_again_where_it_went() {
        let mut m = Mover::new(Some(pace(4.0, 0.0)));
        let to = Vec3::new(0.0, 0.0, 900.0);
        m.remotes.peers.get_mut(&1).unwrap().teleport_hold = Some((to, 0.0));
        assert!(!m.sample(Vec3::new(0.0, 0.0, 450.0), 0.05), "only the teleport's target");
        assert!(m.sample(to + Vec3::X, 0.10));
        m.remotes.peers.get_mut(&1).unwrap().teleport_hold = None;
        assert!(m.sample(to + Vec3::X * 2.0, 0.15), "and it walks on from there");
    }

    /// Rising is bounded by its jump (or its run, if faster); falling by
    /// MAX_FALL_MPS, past any real fall.
    #[test]
    fn a_rise_faster_than_a_jump_is_refused_and_a_fall_is_not() {
        let mut m = Mover::new(Some(pace(4.0, 6.0))); // rises at most 9 m/s
        assert!(!m.sample(Vec3::Y * 25.0, 1.0), "25 m up in a second");
        assert!(m.sample(Vec3::Y * 12.0, 1.05), "within two seconds of its rise");
        assert!(m.sample(Vec3::Y * (12.0 - 100.0), 2.05), "100 m down in a second");
        assert!(!m.sample(Vec3::Y * (12.0 - 500.0), 3.05), "400 m down in one");
    }

    /// Before its replica exists a player moves at PRE_REPLICA_PACE_MPS from
    /// its spawn, and waiting earns at most PRE_REPLICA_STALL_S of it.
    #[test]
    fn before_its_replica_a_player_moves_from_its_spawn_and_waiting_buys_nothing() {
        let mut m = Mover::new(None);
        assert!(!m.sample(Vec3::X * 500.0, 10.0));
        assert!(!m.sample(Vec3::X * 150.0, 10.05), "ten seconds of waiting buy two");
        assert!(m.sample(Vec3::X * 80.0, 10.10));
    }

    /// The host's own systems: entering the world starts the budget at the
    /// spawn point the Welcome gave.
    #[test]
    fn a_player_entering_the_world_is_measured_from_its_spawn() {
        let (mut app, ends) = admission_host(false, &[3]);
        say(&ends, 3, &hello(None));
        app.update();
        say(&ends, 3, &ToHost::WorldReady);
        app.update();
        let host = app.world().resource::<HostSession>();
        let p = &host.peers[&3];
        assert_eq!(p.stage, PeerStage::Ready);
        assert_eq!(p.travel.map(|t| t.pos), Some(Vec3::from_array(host.spawn_for(p.peer))));
    }

    /// A host script snapped its carrier a kilometre: the rider may follow
    /// it there, once, and nowhere else.
    #[test]
    fn a_rider_follows_its_carriers_jump_once() {
        let by = Vec3::X * 1000.0;
        let mut m = Mover::new(Some(Pace { carried_jump: Some((by, 0.0)), ..pace(4.0, 0.0) }));
        assert!(!m.sample(Vec3::Z * 1000.0, 0.05), "only where the carrier went");
        assert!(m.sample(by, 0.10));
        assert!(m.paces.pace(1).unwrap().carried_jump.is_none());
        assert!(!m.sample(by * 2.0, 0.15), "once");
    }

    /// A pace a host script takes away (WalkSpeed 0, a freeze) counts for
    /// MOTION_GRACE_S after, then no longer, and a peer still running at the
    /// old pace is refused. (`track_paces` still floors the pace at the
    /// descriptor's until a rebound body keeps its Humanoid's values: see
    /// docs/networking/PEER_MOTION_PART_B.md.)
    #[test]
    fn a_pace_taken_away_counts_for_its_grace_only() {
        let mut recent = RecentPaces::default();
        assert_eq!(recent.note(0.0, 16.0, 7.0), (16.0, 7.0));
        assert_eq!(recent.note(0.5, 0.0, 7.0), (16.0, 7.0));
        assert_eq!(recent.note(1.4, 0.0, 7.0), (16.0, 7.0));
        assert_eq!(recent.note(1.5, 0.0, 7.0), (0.0, 7.0));
        let frozen = PaceLimit::of(0.0, 0.0, 0.0);
        let mut travel = TravelBudget::start(1.5, Vec3::ZERO, frozen);
        let refused = (1..=40)
            .filter(|&i| !travel.spend(1.5 + i as f64 / 20.0, Vec3::X * 16.0 * i as f32 / 20.0, frozen))
            .count();
        assert!(refused >= 39, "{refused} of 40 refused");
        // Changes past RECENT_PACES fold together, never forgetting the fastest.
        let mut busy = RecentPaces::default();
        for i in 0..50 {
            busy.note(2.0 + i as f64 * 0.01, if i == 3 { 30.0 } else { i as f32 * 0.1 }, 0.0);
        }
        assert!(busy.0.len() <= RECENT_PACES);
        assert_eq!(busy.note(2.5, 0.0, 0.0).0, 30.0);
    }

    // ── Carriers ────────────────────────────────────────────────────────────

    /// The top of a platform centred at `at`, seen under its rider.
    fn sighting(body: Entity, at: Vec3, velocity: Vec3) -> Option<CarrierSighting> {
        Some(CarrierSighting { body, point: at + Vec3::Y * 0.5, transform: Affine3A::from_translation(at), velocity })
    }

    const FRAME: f32 = 1.0 / 60.0;

    /// Ride `body` for `frames` frames as `place_of` moves it; the last speed lent.
    fn ride(track: &mut PeerPace, body: Entity, frames: usize, start: f64, place_of: impl Fn(f32) -> (Vec3, Vec3)) -> f32 {
        let mut speed = 0.0;
        for i in 0..frames {
            let t = i as f32 * FRAME;
            let (at, velocity) = place_of(t);
            speed = track.carried_speed(sighting(body, at, velocity), at + Vec3::Y * 1.5, FRAME, start + t as f64);
        }
        speed
    }

    #[test]
    fn a_part_a_script_tweens_lends_its_speed() {
        let platform = World::new().spawn_empty().id();
        let mut track = PeerPace::new(Vec3::ZERO);
        let speed = ride(&mut track, platform, 30, 0.0, |t| (Vec3::X * 8.0 * t, Vec3::ZERO));
        assert!((speed - 8.0).abs() < 0.01, "{speed}");
    }

    #[test]
    fn a_moving_body_lends_its_velocity_up_to_the_cap() {
        let platform = World::new().spawn_empty().id();
        let mut track = PeerPace::new(Vec3::ZERO);
        assert!((ride(&mut track, platform, 10, 0.0, |_| (Vec3::ZERO, Vec3::Z * 7.0)) - 7.0).abs() < 1e-4);
        let mut fast = PeerPace::new(Vec3::ZERO);
        assert_eq!(ride(&mut fast, platform, 10, 0.0, |_| (Vec3::ZERO, Vec3::Z * 500.0)), CARRIER_CAP_MPS);
    }

    /// Off the platform, its speed carries through a jump's arc for
    /// CARRIER_HOLD_S, then stops.
    #[test]
    fn a_carriers_speed_is_kept_briefly_after_leaving_it() {
        let platform = World::new().spawn_empty().id();
        let mut track = PeerPace::new(Vec3::ZERO);
        ride(&mut track, platform, 30, 0.0, |t| (Vec3::X * 10.0 * t, Vec3::ZERO));
        let left = 30.0 * FRAME as f64;
        let off = |track: &mut PeerPace, after: f64| track.carried_speed(None, Vec3::Y * 3.0, FRAME, left + after);
        assert!((off(&mut track, 0.0) - 10.0).abs() < 0.01, "just off");
        assert!((off(&mut track, 0.3) - 10.0).abs() < 0.01, "mid-jump");
        assert_eq!(off(&mut track, CARRIER_HOLD_S + 0.1), 0.0);
        // A platform that slows lends its speed now, not its fastest.
        let mut slowing = PeerPace::new(Vec3::ZERO);
        let speed = ride(&mut slowing, platform, 120, 0.0, |t| (Vec3::ZERO, Vec3::X * (20.0 - 10.0 * t)));
        assert!((speed - (20.0 - 10.0 * 119.0 * FRAME)).abs() < 0.01, "{speed}");
    }

    /// Falling past a moving part is not riding it.
    #[test]
    fn a_rider_falling_past_a_carrier_borrows_nothing() {
        let platform = World::new().spawn_empty().id();
        let mut track = PeerPace::new(Vec3::ZERO);
        let mut lent = Vec::new();
        for i in 0..20 {
            let t = i as f32 * FRAME;
            let at = Vec3::X * 20.0 * t;
            let rider = Vec3::new(at.x, 1.4 - 15.0 * t, 0.0);
            lent.push(track.carried_speed(sighting(platform, at, Vec3::ZERO), rider, FRAME, t as f64));
        }
        assert!(lent.iter().all(|&s| s == 0.0), "{lent:?}");
    }

    /// A script snapped the platform a kilometre: no speed for it, and the
    /// jump is kept for the rider to follow once.
    #[test]
    fn a_carrier_snapped_far_lends_no_speed() {
        let platform = World::new().spawn_empty().id();
        let mut track = PeerPace::new(Vec3::ZERO);
        track.carried_speed(sighting(platform, Vec3::ZERO, Vec3::ZERO), Vec3::Y * 1.5, FRAME, 0.0);
        let by = Vec3::X * 1000.0;
        let speed = track.carried_speed(sighting(platform, by, Vec3::ZERO), Vec3::Y * 1.5, FRAME, FRAME as f64);
        assert_eq!(speed, 0.0);
        assert_eq!(track.carried_jump.map(|(jump, _)| jump), Some(by));
    }

    /// No avatar carries another, nor anything an avatar holds.
    #[test]
    fn no_avatar_carries_another() {
        let mut world = World::new();
        let avatar = world.spawn_empty().id();
        let hand = world.spawn(ChildOf(avatar)).id();
        let tool = world.spawn(ChildOf(hand)).id();
        let part = world.spawn_empty().id();
        let is_avatar = |e: Entity| e == avatar;
        let parent = |e: Entity| world.get::<ChildOf>(e).map(|c| c.parent());
        assert!(is_avatar_part(avatar, is_avatar, parent));
        assert!(is_avatar_part(tool, is_avatar, parent));
        assert!(!is_avatar_part(part, is_avatar, parent));
    }

    /// The host's pace system with Avian, over replicas the test places
    /// (the minimal-app setup the avatar runtime's own tests use).
    fn physics_host() -> App {
        use avian3d::prelude::{Gravity, PhysicsPlugins};
        bevy::tasks::IoTaskPool::get_or_init(Default::default);
        bevy::tasks::AsyncComputeTaskPool::get_or_init(Default::default);
        bevy::tasks::ComputeTaskPool::get_or_init(Default::default);
        let mut app = App::new();
        app.add_plugins((
            bevy::app::ScheduleRunnerPlugin::default(),
            bevy::time::TimePlugin,
            bevy::transform::TransformPlugin,
            bevy::asset::AssetPlugin::default(),
            bevy::diagnostic::DiagnosticsPlugin,
        ));
        app.init_resource::<avian3d::spatial_query::SpatialQueryDiagnostics>();
        app.init_resource::<avian3d::collider_tree::ColliderTreeDiagnostics>();
        app.init_resource::<avian3d::collision::CollisionDiagnostics>();
        app.init_resource::<avian3d::dynamics::solver::SolverDiagnostics>();
        app.init_asset::<Mesh>();
        app.init_asset::<WorldAsset>();
        app.insert_resource(Time::<Fixed>::from_hz(60.0));
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)));
        app.add_plugins(PhysicsPlugins::default());
        app.insert_resource(Gravity(Vec3::ZERO));
        app.init_resource::<RemoteAvatars>().init_resource::<PeerPaces>().add_systems(Update, track_paces);
        app
    }

    /// A replica for `peer` with its capsule centred at `at`.
    fn place_replica(app: &mut App, peer: PeerId, at: Vec3) -> Entity {
        use avian3d::prelude::{Collider, RigidBody};
        let desc = AvatarDescriptor::default();
        let (metrics, motion) = resolve(&desc, NOMINAL_BIND_HEIGHT_M);
        let collider = Collider::capsule(metrics.capsule_radius, metrics.capsule_cylinder_len);
        let body = AvatarBody { metrics, motion, control: AvatarControl::Remote, metrics_finalised: true };
        let e = app.world_mut().spawn((Transform::from_translation(at), body, desc, RigidBody::Kinematic, collider)).id();
        let mut remotes = app.world_mut().resource_mut::<RemoteAvatars>();
        remotes.add_peer(peer, format!("P{peer}"));
        remotes.peers.get_mut(&peer).unwrap().entity = Some(e);
        e
    }

    /// Through the real ray: a replica on a sliding platform borrows its
    /// speed; one standing on another player's head borrows nothing, though
    /// both move.
    #[test]
    fn a_platform_lends_its_speed_and_a_player_lends_none() {
        use avian3d::prelude::{Collider, LinearVelocity, RigidBody};
        let mut app = physics_host();
        app.world_mut().spawn((
            Transform::from_xyz(0.0, -0.5, 0.0),
            RigidBody::Kinematic,
            Collider::cuboid(40.0, 1.0, 40.0),
            LinearVelocity(Vec3::X * 6.0),
        ));
        let (metrics, motion) = resolve(&AvatarDescriptor::default(), NOMINAL_BIND_HEIGHT_M);
        let half = metrics.capsule_cylinder_len / 2.0 + metrics.capsule_radius;
        place_replica(&mut app, 1, Vec3::new(0.0, half + 0.05, 0.0));
        let below = place_replica(&mut app, 2, Vec3::new(100.0, half, 0.0));
        let above = place_replica(&mut app, 3, Vec3::new(100.0, 3.0 * half + 0.05, 0.0));
        for _ in 0..30 {
            for e in [below, above] {
                app.world_mut().get_mut::<Transform>(e).unwrap().translation.x += 6.0 / 60.0;
            }
            app.update();
        }
        let gravity = -live_gravity(None).y;
        let (top, rise) = own_paces(&motion, gravity);
        let own = PaceLimit::of(top, rise, 0.0);
        let paces = app.world().resource::<PeerPaces>();
        let rider = paces.pace(1).expect("tracked").limit;
        assert!((rider.across - own.across - 6.0).abs() < 0.3, "{} on a 6 m/s platform, {} alone", rider.across, own.across);
        assert_eq!(paces.pace(3).expect("tracked").limit, own, "on a player's head");
    }

    // ── Appearance ──────────────────────────────────────────────────────────

    /// The default look in a hat.
    fn in_hat(id: &str) -> AvatarDescriptor {
        let mut desc = AvatarDescriptor::default();
        desc.set_slot(SlotKind::Hat, Some(ItemId(id.into())));
        desc
    }

    #[test]
    fn a_peer_never_wears_the_hosts_models_or_floods_the_runtime() {
        let identity = AvatarDescriptor::default().resolved_identity();
        let with_rig = |edit: &dyn Fn(&mut RigDefinition)| {
            let mut rig = RigDefinition::builtin(identity);
            edit(&mut rig);
            let desc = AvatarDescriptor { rig: Some(rig), ..AvatarDescriptor::default() };
            parse_appearance(&serde_json::to_string(&desc).unwrap()).expect("still a valid look")
        };
        assert!(with_rig(&|_| {}).rig.is_some(), "a bundled rig is kept");
        assert!(with_rig(&|r| r.body_asset = "space://Characters/hero.glb".into()).rig.is_none(), "a space:// body");
        assert!(with_rig(&|r| r.animations[2] = "space://anims/run.glb".into()).rig.is_none(), "a space:// clip");
        assert!(with_rig(&|r| r.bone_aliases = vec![("x".repeat(65), "hips".into())]).rig.is_none(), "a long alias");
        let many = |r: &mut RigDefinition| {
            r.bone_aliases = (0..=MAX_BONE_ALIASES).map(|i| (format!("b{i}"), format!("t{i}"))).collect();
        };
        assert!(with_rig(&many).rig.is_none(), "too many aliases");
    }

    #[test]
    fn a_look_with_too_many_or_odd_items_is_refused() {
        let look = |slots: Vec<(SlotKind, Option<ItemId>)>| {
            parse_appearance(&serde_json::to_string(&AvatarDescriptor { slots, ..AvatarDescriptor::default() }).unwrap())
        };
        let every = SlotKind::all().map(|k| (k, Some(ItemId(format!("{k:?}-01.v2").to_lowercase()))));
        assert!(look(every.to_vec()).is_some(), "one of each kind");
        assert!(look(vec![(SlotKind::Hat, None)]).is_some(), "an empty slot");
        let two_hats = vec![(SlotKind::Hat, Some(ItemId("cap".into()))), (SlotKind::Hat, Some(ItemId("crown".into())))];
        assert!(look(two_hats).is_none(), "two of one kind");
        let mut nine = every.to_vec();
        nine.push((SlotKind::Hair, None));
        assert!(look(nine).is_none(), "more than the Worker allows");
        for odd in [String::new(), "a b".into(), "../x".into(), "i".repeat(65), "hat\u{202e}".into()] {
            assert!(look(vec![(SlotKind::Hat, Some(ItemId(odd.clone())))]).is_none(), "{odd:?}");
        }
    }

    /// Peer 4 in the world in a cap, with a replica, then in a crown.
    fn rebuilding() -> (App, Entity) {
        let mut app = App::new();
        app.init_resource::<Time>().add_message::<SpawnAvatar>().init_resource::<RemoteAvatars>().add_systems(Update, spawn_replicas);
        let old = app.world_mut().spawn_empty().id();
        let mut remotes = app.world_mut().resource_mut::<RemoteAvatars>();
        remotes.add_peer(4, "Di".into());
        remotes.set_appearance(4, in_hat("cap"));
        remotes.accept_frame(frame(4, 1), 0.0);
        remotes.peers.get_mut(&4).unwrap().entity = Some(old);
        remotes.set_appearance(4, in_hat("crown"));
        (app, old)
    }

    fn spawned(app: &mut App) -> usize {
        app.world_mut().resource_mut::<Messages<SpawnAvatar>>().drain().count()
    }

    #[test]
    fn a_changed_look_replaces_the_body_without_a_gap() {
        let (mut app, old) = rebuilding();
        app.update();
        assert_eq!(spawned(&mut app), 1, "a new body is asked for");
        assert_eq!(app.world().resource::<RemoteAvatars>().peers[&4].entity, Some(old), "the old one stays meanwhile");
        let new = app.world_mut().spawn_empty().id();
        let mut remotes = app.world_mut().resource_mut::<RemoteAvatars>();
        let claimed = remotes.claim(new);
        assert_eq!(claimed, Some(Claimed::Replica { peer: 4, name: "Di".into(), replaced: Some(old) }));
        assert_eq!(remotes.peers[&4].entity, Some(new));
        assert!(remotes.peers[&4].previous_look.is_none());
    }

    #[test]
    fn a_look_the_runtime_cannot_build_keeps_the_old_body() {
        let (mut app, old) = rebuilding();
        app.update();
        assert_eq!(spawned(&mut app), 1);
        app.world_mut().resource_mut::<Time>().advance_by(Duration::from_secs_f64(2.5));
        app.update();
        assert_eq!(spawned(&mut app), 0, "not retried");
        let r = &app.world().resource::<RemoteAvatars>().peers[&4];
        assert_eq!(r.entity, Some(old));
        assert_eq!(r.appearance, Some(in_hat("cap")), "back in the look its body has");
        assert!(!r.respawn);
    }

    #[test]
    fn rebuilds_are_paced_and_never_hold_up_a_first_spawn() {
        let mut app = App::new();
        app.init_resource::<Time>().add_message::<SpawnAvatar>().init_resource::<RemoteAvatars>().add_systems(Update, spawn_replicas);
        let olds: Vec<Entity> = (0..3).map(|_| app.world_mut().spawn_empty().id()).collect();
        {
            let mut remotes = app.world_mut().resource_mut::<RemoteAvatars>();
            for (i, old) in olds.iter().enumerate() {
                let peer = i as PeerId + 1;
                remotes.add_peer(peer, String::new());
                remotes.set_appearance(peer, in_hat("cap"));
                remotes.accept_frame(frame(peer, 1), 0.0);
                remotes.peers.get_mut(&peer).unwrap().entity = Some(*old);
                remotes.set_appearance(peer, in_hat("crown"));
            }
        }
        let claim = |app: &mut App| {
            let new = app.world_mut().spawn_empty().id();
            app.world_mut().resource_mut::<RemoteAvatars>().claim(new).expect("one in flight");
        };
        for _ in 0..REBUILDS_PER_SECOND {
            app.update();
            assert_eq!(spawned(&mut app), 1);
            claim(&mut app);
        }
        app.update();
        assert_eq!(spawned(&mut app), 0, "a third rebuild inside the second waits");
        {
            let mut remotes = app.world_mut().resource_mut::<RemoteAvatars>();
            remotes.add_peer(9, "Newcomer".into());
            remotes.set_appearance(9, in_hat("cap"));
            remotes.accept_frame(frame(9, 1), 0.0);
        }
        app.update();
        assert_eq!(spawned(&mut app), 1, "a first spawn is not held up");
        claim(&mut app);
        app.world_mut().resource_mut::<Time>().advance_by(Duration::from_secs_f64(1.0));
        app.update();
        assert_eq!(spawned(&mut app), 1, "the third rebuild, a second later");
    }

    /// One changed look per APPEARANCE_INTERVAL; a sooner one waits, the
    /// same one again costs nothing, and a flood removes the player.
    #[test]
    fn looks_change_at_most_every_five_seconds_and_a_flood_removes_the_player() {
        let (mut app, ends) = admission_host(false, &[3, 4]);
        {
            let mut host = app.world_mut().resource_mut::<HostSession>();
            for p in host.peers.values_mut() {
                p.stage = PeerStage::Ready;
            }
        }
        {
            let mut remotes = app.world_mut().resource_mut::<RemoteAvatars>();
            remotes.add_peer(1, String::new());
            remotes.add_peer(2, String::new());
        }
        let look = |id: &str| ToHost::Appearance { descriptor_json: serde_json::to_string(&in_hat(id)).unwrap() };
        let hat = |app: &App| {
            let remotes = app.world().resource::<RemoteAvatars>();
            remotes.peers[&1].appearance.as_ref().and_then(|d| d.get_slot(SlotKind::Hat).cloned())
        };
        let violations = |app: &App| app.world().resource::<HostSession>().peers[&3].violations;
        let advance = |app: &mut App, s: f64| app.world_mut().resource_mut::<Time>().advance_by(Duration::from_secs_f64(s));

        say(&ends, 3, &look("cap"));
        app.update();
        assert_eq!(hat(&app), Some(ItemId("cap".into())));

        advance(&mut app, 1.0);
        say(&ends, 3, &look("crown"));
        say(&ends, 3, &look("crown"));
        app.update();
        assert_eq!(hat(&app), Some(ItemId("cap".into())), "a second change inside 5 s waits");
        assert_eq!(violations(&app), 1, "sent twice, counted once");

        advance(&mut app, 4.0);
        app.update();
        assert_eq!(hat(&app), Some(ItemId("crown".into())), "applied when its turn came");
        say(&ends, 3, &look("crown"));
        app.update();
        assert_eq!(violations(&app), 1, "the same look again costs nothing");

        for i in 0..MAX_VIOLATIONS {
            say(&ends, 3, &look(&format!("hat{i}")));
        }
        app.update();
        let (_, removed) = sent_to(&ends, 3);
        assert!(removed, "a flood of looks removes the player");
    }

    /// Looks sent while the world downloads are paced the same way, and the
    /// newest is the one the player enters the world in.
    #[test]
    fn looks_sent_while_downloading_are_paced_too() {
        let (mut app, ends) = admission_host(false, &[3]);
        say(&ends, 3, &hello(None));
        app.update();
        let look = |id: &str| ToHost::Appearance { descriptor_json: serde_json::to_string(&in_hat(id)).unwrap() };
        let violations = |app: &App| app.world().resource::<HostSession>().peers[&3].violations;
        say(&ends, 3, &look("cap"));
        app.update();
        app.world_mut().resource_mut::<Time>().advance_by(Duration::from_secs_f64(1.0));
        say(&ends, 3, &look("crown"));
        app.update();
        assert_eq!(violations(&app), 1, "a second look inside 5 s, while downloading");
        say(&ends, 3, &ToHost::WorldReady);
        app.update();
        let remotes = app.world().resource::<RemoteAvatars>();
        let hat = remotes.peers[&1].appearance.as_ref().and_then(|d| d.get_slot(SlotKind::Hat).cloned());
        assert_eq!(hat, Some(ItemId("crown".into())), "enters the world in its newest look");
        for i in 0..MAX_VIOLATIONS {
            say(&ends, 3, &look(&format!("hat{i}")));
        }
        app.update();
        assert!(sent_to(&ends, 3).1, "a flood of looks removes the player");
    }

    /// A burst of samples in one frame (a stall's backlog over a stream) is
    /// read as its newest: kept and relayed once, whatever order it came in.
    #[test]
    fn a_burst_in_one_frame_is_read_as_its_newest_sample() {
        let (mut app, ends) = admission_host(false, &[3, 4]);
        {
            let mut host = app.world_mut().resource_mut::<HostSession>();
            for p in host.peers.values_mut() {
                p.stage = PeerStage::Ready;
                p.travel = Some(TravelBudget::start(0.0, Vec3::ZERO, PaceLimit::before_replica()));
            }
        }
        {
            let mut remotes = app.world_mut().resource_mut::<RemoteAvatars>();
            remotes.add_peer(1, String::new());
            remotes.add_peer(2, String::new());
        }
        for seq in [5, 9, 7] {
            let sample = AvatarFrame { position: [seq as f32, 0.0, 0.0], ..frame(1, seq) };
            ends.events.send(LinkEvent::Datagram { conn: 3, bytes: encode_datagram(&Datagram::Avatar(sample)) }).unwrap();
        }
        app.update();
        assert_eq!(app.world().resource::<RemoteAvatars>().peers[&1].latest.map(|f| f.seq), Some(9));
        let relayed: Vec<u32> = ends
            .commands
            .try_iter()
            .filter_map(|c| match c {
                LinkCommand::Datagram { conn: 4, bytes } => match decode_datagram(&bytes) {
                    Ok(Datagram::Avatar(f)) => Some(f.seq),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(relayed, vec![9]);
    }

    /// A joining Player holds SpaceOwnsMotion from the frame it joins: its
    /// avatar is asked for only once the world has arrived, after the host's
    /// Welcome and the download, so the first avatar of a join already moves
    /// at the Space's pace. Leaving drops it.
    #[test]
    fn a_joining_player_holds_space_owns_motion_from_the_start() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.add_systems(Update, |mut commands: Commands, mut joined: Local<bool>| {
            if !*joined {
                *joined = true;
                let (link, _ends) = link_pair();
                begin_join(&mut commands, link, "Ada", None);
            }
        });
        app.update();
        assert!(app.world().contains_resource::<PlayerSession>());
        assert!(app.world().contains_resource::<SpaceOwnsMotion>(), "in place the frame it joins");
        app.world_mut().run_system_once(|mut commands: Commands| end_session(&mut commands)).unwrap();
        assert!(!app.world().contains_resource::<SpaceOwnsMotion>(), "gone with the session");
    }

    /// The Player's side of a correction: applied to the local avatar, but
    /// never a NaN or a place outside the world, never while seated, and
    /// never over a host teleport that came after it.
    #[test]
    fn a_player_takes_only_sane_corrections() {
        assert_eq!(usable_correction([1.0, 2.0, 3.0]), Some(Vec3::new(1.0, 2.0, 3.0)));
        assert_eq!(usable_correction([f32::NAN, 0.0, 0.0]), None);
        assert_eq!(usable_correction([0.0, f32::INFINITY, 0.0]), None);
        assert_eq!(usable_correction([2.0e5, 0.0, 0.0]), None);

        let mut local = LocalAvatarNet::default();
        let walk = |local: &mut LocalAvatarNet, x: f32, now: f64| local.take_correction(Vec3::X * x, false, now);
        assert_eq!(walk(&mut local, 0.0, 0.0), None);
        local.correction = Some(Vec3::ONE);
        assert_eq!(local.take_correction(Vec3::X * 0.1, true, 0.1), None, "seated: the seat places it");
        local.correction = Some(Vec3::ONE);
        assert_eq!(walk(&mut local, 0.2, 0.2), Some(Vec3::ONE));
        // The host teleported it (a jump no step makes); an older correction
        // arriving within TELEPORT_HOLD would undo that.
        assert_eq!(walk(&mut local, 300.0, 0.3), None);
        local.correction = Some(Vec3::ONE);
        assert_eq!(walk(&mut local, 300.1, 0.3 + TELEPORT_HOLD / 2.0), None);
        local.correction = Some(Vec3::X * 300.0);
        assert_eq!(walk(&mut local, 300.2, 0.3 + TELEPORT_HOLD), Some(Vec3::X * 300.0));
    }

    // ── Session size (Players.MaxPlayers) ───────────────────────────────────

    /// The session holds Players.MaxPlayers, everyone counted: with the host
    /// playing, a 4-player Space takes 3 joiners and turns the next away with
    /// the count; a host with no character leaves its place to a joiner. The
    /// heartbeat's numbers count the same way.
    #[test]
    fn a_full_session_turns_the_next_player_away() {
        let opened = |host_plays: bool, arrivals: u64| {
            let (link, ends) = link_pair();
            let mut host = HostSession::new(HostConfig { max_players: 4, ..host_config() });
            host.set_host_plays(host_plays);
            let mut app = App::new();
            app.init_resource::<Time>()
                .init_resource::<RemoteAvatars>()
                .init_resource::<PeerPaces>()
                .init_resource::<PeerInputs>()
                .add_message::<NetNotice>()
                .add_message::<RemoteArrived>()
                .add_message::<TracksArrived>()
                .insert_resource(link)
                .insert_resource(host)
                .add_systems(Update, host_pump);
            for conn in 1..=arrivals {
                ends.events.send(LinkEvent::Opened { conn, remote: format!("10.0.0.{conn}:1"), datagrams: true }).unwrap();
            }
            app.update();
            (app, ends)
        };

        let (app, ends) = opened(true, 4);
        assert_eq!(app.world().resource::<HostSession>().peers.len(), 3, "the host plus 3");
        let (msgs, closed) = sent_to(&ends, 4);
        assert!(closed, "the 4th joiner is turned away");
        assert!(
            matches!(msgs.as_slice(), [ToPlayer::Refused { reason }] if reason == "This session is full (4 of 4 players)."),
            "{msgs:?}"
        );

        let (app, ends) = opened(false, 5);
        assert_eq!(app.world().resource::<HostSession>().peers.len(), 4, "no host character: 4 joiners");
        assert!(sent_to(&ends, 5).1);

        let mut host = HostSession::new(HostConfig { max_players: 4, ..host_config() });
        host.set_host_plays(true);
        let mut ready = HostPeer::new(1, String::new(), true, 0.0);
        ready.stage = PeerStage::Ready;
        host.peers.insert(1, ready);
        host.peers.insert(2, HostPeer::new(2, String::new(), true, 0.0));
        assert_eq!((host.player_count(), host.max_players()), (2, 4), "Live shows 2/4: the host and one in the world");
    }
}
