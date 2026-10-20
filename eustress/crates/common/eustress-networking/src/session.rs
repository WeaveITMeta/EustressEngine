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

use bevy::prelude::*;
use crossbeam_channel::{unbounded, Receiver, Sender};
use serde::Serialize;

use eustress_common::avatar::seat::AvatarSeated;
use eustress_common::avatar::spawn::{AvatarBody, AvatarIntent, AvatarLocomotion};
use eustress_common::avatar::{
    AvatarControl, AvatarDescriptor, AvatarSystems, LocalAvatar, SpawnAvatar, SpawnedByAvatarRuntime,
};
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
    decode_body, decode_datagram, encode_datagram, encode_frame, sanitize_chat, sanitize_name, sanitize_receipts,
    sanitize_ticket, AvatarFrame, Datagram, Hello, PeerId, PeerInfo, ToHost, ToPlayer, Welcome, CHUNK_PIECE_BYTES,
    HOST_PEER, MAX_APPEARANCE_BYTES, MAX_SIM_ID_CHARS, PROTOCOL_VERSION,
};

/// Avatar samples per second, each direction.
pub const AVATAR_SEND_HZ: f32 = 20.0;
/// World bytes a host hands one player's stream per frame.
const CHUNK_BUDGET_PER_FRAME: usize = 1024 * 1024;
/// Fastest an avatar may move before the host drops the sample, in m/s.
const MAX_AVATAR_SPEED: f32 = 80.0;
/// Distance an avatar may cover in one sample regardless of speed, so a
/// teleport by a script on the sender's side is not mistaken for cheating
/// when samples are close together.
const SPEED_SLACK_M: f32 = 6.0;
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
    PeerIdentity { peer: PeerId, ticket: String },
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
    last_sample: Option<(f64, Vec3)>,
    violations: u32,
    violation_window: f64,
    chat_times: VecDeque<f64>,
    /// The identity ticket its Hello carried, unverified.
    identity: Option<String>,
    /// Purchase prompts sent to this player and not yet answered.
    prompts: HashSet<u32>,
    receipt_times: VecDeque<f64>,
    /// Its catch-up world frames went out; live ones may follow.
    caught_up: bool,
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
            last_sample: None,
            violations: 0,
            violation_window: now,
            chat_times: VecDeque::new(),
            identity: None,
            prompts: HashSet::new(),
            receipt_times: VecDeque::new(),
            caught_up: false,
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
        }
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

    /// Players in the world.
    pub fn player_count(&self) -> usize {
        self.peers.values().filter(|p| p.stage == PeerStage::Ready).count()
    }

    /// Players the session takes besides the host.
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
    /// This player's identity ticket, sent in Hello.
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
    respawn: bool,
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

/// Every other participant's avatar, as this machine knows it.
#[derive(Resource, Default)]
pub struct RemoteAvatars {
    peers: HashMap<PeerId, Remote>,
    /// A replica requested but not yet seen: (peer, spawn point, when).
    in_flight: Option<(PeerId, Vec3, f64)>,
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
            placed: None,
            teleport_hold: None,
        });
    }

    /// Forget a peer; returns its replica for the caller to despawn.
    fn remove_peer(&mut self, peer: PeerId) -> Option<Entity> {
        self.peers.remove(&peer).and_then(|r| r.entity)
    }

    fn set_appearance(&mut self, peer: PeerId, descriptor: AvatarDescriptor) {
        if let Some(r) = self.peers.get_mut(&peer) {
            let changed = r.appearance.as_ref() != Some(&descriptor);
            r.appearance = Some(descriptor);
            // A replica already in the world with the old body is rebuilt.
            r.respawn = changed && r.entity.is_some();
        }
    }

    /// Keep the newest sample. `seq` wraps, so "newer" is a wrapping compare.
    /// Every replica's spawn point, pose and intent come from the sample kept
    /// here, so only a finite one within [`WORLD_LIMIT`] is kept.
    fn accept_frame(&mut self, frame: AvatarFrame, now: f64) {
        if !frame.is_finite() || Vec3::from_array(frame.position).abs().max_element() >= WORLD_LIMIT {
            return;
        }
        if let Some(r) = self.peers.get_mut(&frame.peer) {
            if let Some(prev) = r.latest {
                let ahead = frame.seq.wrapping_sub(prev.seq);
                if ahead == 0 || ahead > u32::MAX / 2 {
                    return; // duplicate or reordered
                }
            }
            r.latest = Some(frame);
            r.received_at = now;
        }
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
            // Idempotent; the avatar runtime registers it too. Registered here
            // so the replica spawner's writer is valid in any shell.
            .add_message::<SpawnAvatar>()
            .init_resource::<RemoteAvatars>()
            .init_resource::<LocalAvatarNet>()
            .init_resource::<PeerInputs>();

        // Every system is gated on a session existing, and Bevy evaluates run
        // conditions before validating parameters, so a shell that never
        // starts a session (or has no avatar runtime, like the headless
        // engine) pays nothing and never trips a missing-resource error.
        app.add_systems(
            Update,
            (
                host_pump.run_if(hosting),
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
            (correct_replicas.run_if(in_session), send_local_avatar.run_if(in_session))
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
}

fn end_session(commands: &mut Commands) {
    // Removing the link drops it, which tells the transport to shut down
    // after any commands already queued.
    commands.remove_resource::<NetLink>();
    commands.remove_resource::<HostSession>();
    commands.remove_resource::<PlayerSession>();
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
) {
    let now = time.elapsed_secs_f64();
    for _ in 0..MAX_EVENTS_PER_FRAME {
        let Some(event) = link.poll() else { break };
        match event {
            LinkEvent::Listening { port, cert_sha256 } => {
                host.port = port;
                host.pin = Some(cert_sha256);
                notices.write(NetNotice::Hosting { port, pin: cert_sha256 });
            }
            LinkEvent::Opened { conn, remote, datagrams } => {
                if host.peers.len() >= host.config.max_players as usize {
                    link.send(conn, &ToPlayer::Refused { reason: "This session is full.".into() });
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
                Ok(msg) => host_message(&mut commands, &link, &mut host, &mut remotes, &mut notices, conn, msg, now),
                Err(e) => kick(&link, &mut host, conn, &format!("unreadable message: {e}")),
            },
            LinkEvent::Datagram { conn, bytes } => match decode_datagram(&bytes) {
                Ok(Datagram::Avatar(frame)) => host_avatar_frame(&link, &mut host, &mut remotes, conn, frame, now),
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
            let spawn = host.spawn_for(peer);
            let welcome = Welcome {
                protocol: PROTOCOL_VERSION,
                peer,
                host_name: host.config.host_name.clone(),
                world: host.config.world.clone(),
                spawn,
                max_players: host.config.max_players,
                sim_id: host.config.sim_id.clone(),
                host_identity: host.identity.clone(),
                tick: tick_at(now),
            };
            let p = host.peers.get_mut(&conn).expect("present above");
            p.name = name.clone();
            p.stage = PeerStage::Downloading;
            p.identity = hello.identity.as_deref().and_then(sanitize_ticket);
            // The player's own words, so printable and short before logging.
            let app: String = hello.engine_version.chars().filter(|c| !c.is_control()).take(64).collect();
            info!("net: {name} ({}) joined as peer {peer}, running {}", p.remote, app.trim());
            link.send(conn, &ToPlayer::Welcome(welcome));
        }
        ToHost::RequestChunks { hashes } => {
            if stage == PeerStage::AwaitingHello {
                return kick(link, host, conn, "requested the world before Hello");
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
            let (peer, name, appearance, identity) = {
                let p = host.peers.get_mut(&conn).expect("present above");
                p.stage = PeerStage::Ready;
                (p.peer, p.name.clone(), p.appearance.clone(), p.identity.clone())
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
            let Some(desc) = parse_appearance(&descriptor_json) else {
                warn!("net: ignored an invalid appearance from connection {conn}");
                return;
            };
            let peer = {
                let p = host.peers.get_mut(&conn).expect("present above");
                p.appearance = Some(descriptor_json.clone());
                p.peer
            };
            if stage == PeerStage::Ready {
                for (other, _) in host.ready() {
                    if other != conn {
                        link.send(other, &ToPlayer::Appearance { peer, descriptor_json: descriptor_json.clone() });
                    }
                }
                remotes.set_appearance(peer, desc);
            }
        }
        ToHost::Chat { text } => {
            if stage != PeerStage::Ready {
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
        ToHost::Avatar(frame) => host_avatar_frame(link, host, remotes, conn, frame, now),
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
        ToHost::Remote(_) | ToHost::Input(_) | ToHost::Tracks(_) => {}
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
fn host_avatar_frame(
    link: &NetLink,
    host: &mut HostSession,
    remotes: &mut RemoteAvatars,
    conn: ConnId,
    mut frame: AvatarFrame,
    now: f64,
) {
    let Some(p) = host.peers.get_mut(&conn) else { return };
    if p.stage != PeerStage::Ready {
        return;
    }
    let pos = Vec3::from_array(frame.position);
    let plausible = frame.is_finite()
        && pos.abs().max_element() < WORLD_LIMIT
        && p.last_sample.is_none_or(|(t, last)| {
            let dt = (now - t).max(0.0) as f32;
            pos.distance(last) <= MAX_AVATAR_SPEED * dt + SPEED_SLACK_M
        });
    if !plausible {
        if p.violation(now) {
            let name = p.name.clone();
            warn!("net: removing {name}: {MAX_VIOLATIONS} impossible movements in {VIOLATION_WINDOW} s");
            return kick(link, host, conn, "movement the host could not accept");
        }
        return;
    }
    p.last_sample = Some((now, pos));
    frame.peer = p.peer; // never trust the sender's claim of who it is
    remotes.accept_frame(frame, now);

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

fn kick(link: &NetLink, host: &mut HostSession, conn: ConnId, reason: &str) {
    link.send(conn, &ToPlayer::Kicked { reason: reason.to_string() });
    link.close(conn, reason);
    // The transport reports Closed, which cleans up and tells everyone.
    if let Some(p) = host.peers.get_mut(&conn) {
        p.outbox.clear();
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
                link.send(
                    HOST_CONN,
                    &ToHost::Hello(Hello {
                        protocol: PROTOCOL_VERSION,
                        name: player.name.clone(),
                        engine_version,
                        identity: player.identity.clone(),
                    }),
                );
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
                Err(e) => Err(format!("the host sent an unreadable message: {e}")),
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
            LinkEvent::Closed { reason, .. } => Err(format!("the host closed the connection: {reason}")),
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
        ToPlayer::World(_) | ToPlayer::Motion(_) | ToPlayer::RemoteReply(_) => {}
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

fn parse_appearance(json: &str) -> Option<AvatarDescriptor> {
    if json.len() > MAX_APPEARANCE_BYTES {
        return None;
    }
    let desc: AvatarDescriptor = serde_json::from_str(json).ok()?;
    desc.validate().ok()?;
    Some(desc)
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
/// both known. One at a time, so each new avatar can be matched to its peer.
fn spawn_replicas(
    mut commands: Commands,
    time: Res<Time>,
    mut remotes: ResMut<RemoteAvatars>,
    mut spawn: MessageWriter<SpawnAvatar>,
) {
    let now = time.elapsed_secs_f64();
    for r in remotes.peers.values_mut() {
        if r.respawn {
            if let Some(e) = r.entity.take() {
                commands.entity(e).try_despawn();
            }
            r.respawn = false;
        }
    }
    if let Some((_, _, since)) = remotes.in_flight {
        if now - since < 2.0 {
            return;
        }
        // The runtime never produced it (for instance, it rejected the
        // descriptor); the peer is retried on a later frame.
        remotes.in_flight = None;
    }
    let next = remotes.peers.iter().find_map(|(peer, r)| {
        let (Some(desc), Some(frame)) = (&r.appearance, r.latest) else { return None };
        r.entity.is_none().then(|| (*peer, desc.clone(), frame))
    });
    if let Some((peer, desc, frame)) = next {
        // A frame carries the capsule centre; spawn takes the feet. Starting a
        // metre low is corrected on the first frame after spawn.
        let at = Vec3::from_array(frame.position) - Vec3::Y;
        spawn.write(SpawnAvatar::new(desc, at).with_yaw(frame.yaw).with_control(AvatarControl::Remote));
        remotes.in_flight = Some((peer, at, now));
    }
}

/// Claim the avatar the runtime just built for the peer in flight.
fn tag_replicas(
    mut commands: Commands,
    mut remotes: ResMut<RemoteAvatars>,
    fresh: Query<
        (Entity, &AvatarBody, &Transform),
        (Added<SpawnedByAvatarRuntime>, Without<LocalAvatar>, Without<NetReplica>),
    >,
) {
    let Some((peer, at, _)) = remotes.in_flight else { return };
    for (entity, body, tf) in &fresh {
        if body.control != AvatarControl::Remote || tf.translation.distance(at) > 6.0 {
            continue;
        }
        remotes.in_flight = None;
        match remotes.peers.get_mut(&peer) {
            Some(r) => {
                r.entity = Some(entity);
                commands.entity(entity).insert((NetReplica { peer }, Name::new(format!("Player: {}", r.name))));
            }
            // The peer left while its avatar was being built.
            None => commands.entity(entity).try_despawn(),
        }
        break;
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
    replicas: Query<Entity, With<NetReplica>>,
) {
    for e in &replicas {
        commands.entity(e).try_despawn();
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
}
