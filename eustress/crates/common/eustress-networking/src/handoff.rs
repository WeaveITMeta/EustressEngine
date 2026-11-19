//! # Host handoff: state transfer with one owner at a time
//!
//! An entity (a car with its riders, or a player on foot) that crosses from
//! one host's area into another's is handed over whole. The protocol and its
//! failure table are in `docs/architecture/HOST_HANDOFF_STATE_TRANSFER.md`.
//!
//! ```text
//! source                                         destination
//!   begin: Offer{id, epoch, snapshot} ──────────▶ checks, stages it (not in its world)
//!   (still simulates it)              ◀────────── Accept, or Refuse{reason}
//!   Finalize: freezes it, commit():
//!   Commit{id, epoch, final snapshot} ──────────▶ spawns it under the same ids,
//!   (keeps the frozen copy)                       simulates it from now: owner
//!                                     ◀────────── Committed
//!   Release: removes its copy, riders reconnect
//! ```
//!
//! Authority passes on Commit and nowhere else: the source gives it up when
//! it sends Commit, the destination takes it when Commit arrives. In the one
//! trip between, the entity is frozen and its state is the Commit itself; no
//! host steps it. The source keeps custody (the frozen copy) until
//! `Committed`, so a destination that dies before taking it never loses it,
//! and a source never resumes on a timer once it has sent Commit: only a
//! refusal or the control plane declaring the destination gone
//! ([`Handoffs::peer_lost`]) gives it back.
//!
//! Every message carries the handoff id and the entity's epoch. Duplicates,
//! late and out-of-order messages are answered from what the receiving side
//! already decided and never change that decision.
//!
//! Pure state: no IO, no clocks (callers pass milliseconds), no threads. The
//! messages are encoded with the wire's own framing ([`crate::wire`]) and the
//! instances with the replication layer's [`SpawnOp`] and [`WireValue`], so
//! there is one codec. The host-to-host transport, the engine glue (Avian
//! state in and out, seats, a rider's reconnect) and the area that decides a
//! crossing are not here; [`HandoffRequest`] is the seam the streaming layer
//! asks through.

use std::collections::{HashMap, HashSet};

use bevy::prelude::Resource;
use eustress_common::datamodel::{DataModel, InstanceId};
use serde::{Deserialize, Serialize};

use crate::repl::{HostReplicator, NetId, Replica, ReplOp, SpawnOp, ValueLimits, WorldFrame};
use crate::wire::{decode_body, encode_frame, WireError, MAX_FRAME_BYTES};

/// Largest handoff message, body only: a quarter of the wire's frame limit,
/// the same budget as a world frame. A car with its riders is kilobytes.
pub const MAX_HANDOFF_BYTES: usize = MAX_FRAME_BYTES / 4;
/// Instances one snapshot may carry.
pub const MAX_HANDOFF_INSTANCES: usize = 4096;
/// Physics bodies one snapshot may carry.
pub const MAX_HANDOFF_BODIES: usize = 512;
/// Riders one snapshot may carry.
pub const MAX_HANDOFF_RIDERS: usize = 64;
/// An account id is at most this long, printable ASCII.
pub const MAX_ACCOUNT_CHARS: usize = 64;
/// A class name is at most this long.
const MAX_CLASS_CHARS: usize = 64;
/// An instance name or a tag is at most this many bytes.
const MAX_NAME_BYTES: usize = 256;
/// Properties, attributes or tags on one instance.
const MAX_FIELDS: usize = 512;

/// A host's origin in its universe: the high bits of every runtime id it
/// mints ([`NetId::origin`]), and the address book key for its handoffs. 0 is
/// a host on its own, which never hands anything off.
pub type Origin = u32;

/// One handoff, unique across hosts: the source's origin and its own count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct HandoffId(pub u64);

impl HandoffId {
    const SEQ_BITS: u32 = 40;

    pub fn new(origin: Origin, seq: u64) -> Self {
        HandoffId(((origin as u64) << Self::SEQ_BITS) | (seq & ((1 << Self::SEQ_BITS) - 1)))
    }

    /// The host that began it, the only one that may send its Offer, Commit
    /// and Abort.
    pub fn origin(self) -> Origin {
        (self.0 >> Self::SEQ_BITS) as Origin
    }
}

/// How a body moves, as Avian's `RigidBody` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BodyKind {
    Dynamic,
    Kinematic,
    Static,
}

/// One physics body at the snapshot's tick, at Avian's own precision, so the
/// destination continues the motion instead of starting it at rest.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HandoffBody {
    /// The instance the body belongs to; one of the snapshot's.
    pub id: NetId,
    pub position: [f32; 3],
    /// Unit quaternion (x, y, z, w).
    pub rotation: [f32; 4],
    /// Linear velocity, m/s.
    pub linear: [f32; 3],
    /// Angular velocity in world axes, rad/s.
    pub angular: [f32; 3],
    pub kind: BodyKind,
    /// Avian had put it to sleep.
    pub sleeping: bool,
}

impl HandoffBody {
    fn is_valid(&self) -> bool {
        let finite = |xs: &[f32]| xs.iter().all(|x| x.is_finite());
        let len2: f32 = self.rotation.iter().map(|x| x * x).sum();
        finite(&self.position) && finite(&self.rotation) && finite(&self.linear) && finite(&self.angular)
            && (len2 - 1.0).abs() < 1e-3
    }
}

/// A player riding the entity (or the player who is the entity). Players are
/// keyed by account across hosts: session peer ids belong to one host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rider {
    /// The account the rider signed in as, the key its reconnect is matched on.
    pub account: String,
    /// The rider's `Player` instance on the source. The destination gives
    /// the `Player` it makes for this account on reconnect the same id.
    pub player: NetId,
    /// The rider's character, one of the snapshot's instances.
    pub character: Option<NetId>,
    /// The seat it sits in, one of the snapshot's instances; `None` on foot.
    pub seat: Option<NetId>,
}

/// Everything the destination needs to continue an entity: its instances as
/// players are sent them (properties, attributes, tags: the script-visible
/// state), its bodies' physics state, its owner and its riders.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoffSnapshot {
    /// The entity: the first of `instances`.
    pub root: NetId,
    /// The source's tick when it was taken. Ticks are per host; this is for
    /// logs and ordering on the source, never compared with the destination's.
    pub tick: u64,
    /// Parents before children. An instance whose parent is not in the
    /// snapshot heads a subtree (the entity, a rider's character), and that
    /// parent must exist on the destination (a service, or a scene instance
    /// both hosts load).
    pub instances: Vec<SpawnOp>,
    pub bodies: Vec<HandoffBody>,
    /// The account that owns the entity (who spawned the car), when one does.
    pub owner: Option<String>,
    pub riders: Vec<Rider>,
}

fn account_ok(a: &str) -> bool {
    !a.is_empty() && a.len() <= MAX_ACCOUNT_CHARS && a.bytes().all(|b| b.is_ascii_graphic())
}

impl HandoffSnapshot {
    /// Within the bounds and well formed: unique ids, parents first, finite
    /// bodies, riders that name its own instances, values a host accepts.
    /// The encoded size is checked when a message is encoded.
    pub fn check(&self) -> Result<(), Refusal> {
        let bad = Err(Refusal::Invalid);
        if self.instances.is_empty() || self.instances.len() > MAX_HANDOFF_INSTANCES {
            return bad;
        }
        if self.bodies.len() > MAX_HANDOFF_BODIES || self.riders.len() > MAX_HANDOFF_RIDERS {
            return bad;
        }
        if self.instances[0].id != self.root {
            return bad;
        }
        let limits = ValueLimits::default();
        let all: HashSet<NetId> = self.ids().collect();
        let mut seen: HashSet<NetId> = HashSet::with_capacity(self.instances.len());
        for s in &self.instances {
            if s.id.is_none() || s.id == NetId::ROOT || s.parent.is_none() || s.parent == s.id {
                return bad;
            }
            // A parent inside the snapshot comes first; one outside it heads
            // a subtree and is looked up on the destination.
            if all.contains(&s.parent) && !seen.contains(&s.parent) {
                return bad;
            }
            if !seen.insert(s.id) {
                return bad;
            }
            if s.class.is_empty() || s.class.len() > MAX_CLASS_CHARS || s.name.len() > MAX_NAME_BYTES {
                return bad;
            }
            if s.props.len() > MAX_FIELDS || s.attributes.len() > MAX_FIELDS || s.tags.len() > MAX_FIELDS {
                return bad;
            }
            if s.tags.iter().any(|t| t.len() > MAX_NAME_BYTES) {
                return bad;
            }
            for (k, v) in s.props.iter().chain(s.attributes.iter()) {
                if k.len() > MAX_NAME_BYTES || v.check(&limits).is_err() {
                    return bad;
                }
            }
        }
        let mut bodies: HashSet<NetId> = HashSet::new();
        for b in &self.bodies {
            if !seen.contains(&b.id) || !bodies.insert(b.id) || !b.is_valid() {
                return bad;
            }
        }
        if self.owner.as_deref().is_some_and(|o| !account_ok(o)) {
            return bad;
        }
        let mut accounts: HashSet<&str> = HashSet::new();
        for r in &self.riders {
            if !account_ok(&r.account) || !accounts.insert(r.account.as_str()) {
                return bad;
            }
            if !r.player.is_runtime() || seen.contains(&r.player) {
                return bad;
            }
            if r.character.is_some_and(|c| !seen.contains(&c)) || r.seat.is_some_and(|s| !seen.contains(&s)) {
                return bad;
            }
        }
        Ok(())
    }

    /// Every instance id it carries.
    pub fn ids(&self) -> impl Iterator<Item = NetId> + '_ {
        self.instances.iter().map(|s| s.id)
    }
}

/// Why a destination will not take an entity. Pinned on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Refusal {
    /// It has no room for it now.
    Full,
    /// One of the snapshot's ids names something else there.
    IdTaken,
    /// It has seen the entity at a newer epoch: the message is old.
    Stale,
    /// It owns the entity already.
    AlreadyHeld,
    /// Another handoff of the entity is open there.
    Busy,
    /// The snapshot breaks a rule: size, order, values.
    Invalid,
    /// A Commit for a handoff it does not hold (never staged, aborted, or
    /// dropped when its wait ran out).
    NotHeld,
    /// A parent the snapshot hangs under is not in its world.
    UnknownParent,
    /// It takes no handoffs now (shutting down, or not a shard host).
    Closed,
}

/// Host to host, one per frame on the shard channel. Pinned on the wire by
/// `handoff_tags_never_move`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HandoffMsg {
    /// Source: take this entity. The source still owns and simulates it.
    Offer { id: HandoffId, epoch: u64, snapshot: HandoffSnapshot },
    /// Destination: staged; send the Commit.
    Accept { id: HandoffId, epoch: u64 },
    /// Destination: no. Answers an Offer, or a Commit it cannot take.
    Refuse { id: HandoffId, epoch: u64, reason: Refusal },
    /// Source: it is yours, as of this final snapshot. The source has frozen
    /// it and owns it no longer.
    Commit { id: HandoffId, epoch: u64, snapshot: HandoffSnapshot },
    /// Destination: it holds the entity and simulates it.
    Committed { id: HandoffId, epoch: u64 },
    /// Source: forget the Offer. Never sent after a Commit.
    Abort { id: HandoffId, epoch: u64 },
}

impl HandoffMsg {
    pub fn id(&self) -> HandoffId {
        match self {
            HandoffMsg::Offer { id, .. }
            | HandoffMsg::Accept { id, .. }
            | HandoffMsg::Refuse { id, .. }
            | HandoffMsg::Commit { id, .. }
            | HandoffMsg::Committed { id, .. }
            | HandoffMsg::Abort { id, .. } => *id,
        }
    }

    pub fn epoch(&self) -> u64 {
        match self {
            HandoffMsg::Offer { epoch, .. }
            | HandoffMsg::Accept { epoch, .. }
            | HandoffMsg::Refuse { epoch, .. }
            | HandoffMsg::Commit { epoch, .. }
            | HandoffMsg::Committed { epoch, .. }
            | HandoffMsg::Abort { epoch, .. } => *epoch,
        }
    }
}

/// One frame (`u32` length, bincode body) as [`crate::wire::encode_frame`]
/// writes it, refused past [`MAX_HANDOFF_BYTES`].
pub fn encode_message(msg: &HandoffMsg) -> Result<Vec<u8>, WireError> {
    let frame = encode_frame(msg)?;
    if frame.len() - 4 > MAX_HANDOFF_BYTES {
        return Err(WireError::TooLarge(frame.len() - 4));
    }
    Ok(frame)
}

/// A frame body read off the shard channel. Bounded before it is decoded,
/// and values decode under the wire's depth bound
/// ([`crate::repl::value::MAX_DECODE_DEPTH`]).
pub fn decode_message(body: &[u8]) -> Result<HandoffMsg, WireError> {
    if body.len() > MAX_HANDOFF_BYTES {
        return Err(WireError::TooLarge(body.len()));
    }
    decode_body(body)
}

/// The host a handoff goes to: its origin, and how its riders reach it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Neighbour {
    pub origin: Origin,
    /// Its server id on the listing (`/live` rows).
    pub server_id: String,
    /// Its join link, for riders.
    pub link: String,
    /// Its certificate pin: the audience of each rider's pre-issued ticket.
    pub cert_sha256: String,
}

/// One server row as `GET /api/simulations/{id}/live` returns it, in the
/// Worker's order (`rankServers` in `live.mjs`: not full first, then by score).
/// `area` and `origin` are the shard fields that row gains with sharding.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct LiveRow {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub full: bool,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub cert_sha256: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub origin: Option<u32>,
}

/// The neighbour a handoff into `area` goes to: the first row, in the order
/// the Worker ranked them, that serves that area, is not full, is not this
/// server (`me`), and can be reached and pinned. The Worker's ranking is the
/// only ranking; this never re-sorts.
pub fn pick_neighbour(rows: &[LiveRow], area: &str, me: &str) -> Option<Neighbour> {
    rows.iter().find_map(|r| {
        if r.id == me || r.full || r.area.as_deref() != Some(area) {
            return None;
        }
        let origin = r.origin.filter(|o| *o != 0 && *o <= NetId::MAX_ORIGIN)?;
        let link = r.link.clone().filter(|l| !l.is_empty())?;
        let pin = r.cert_sha256.clone().filter(|p| p.len() == 64 && p.bytes().all(|b| b.is_ascii_hexdigit()))?;
        Some(Neighbour { origin, server_id: r.id.clone(), link, cert_sha256: pin.to_ascii_lowercase() })
    })
}

/// The seam the streaming layer asks through: `root` is leaving this host's
/// area for `area`. The session glue picks the neighbour
/// ([`pick_neighbour`]), takes the snapshot ([`snapshot_of`]) and calls
/// [`Handoffs::begin`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffRequest {
    pub root: NetId,
    pub area: String,
}

/// Timers, in milliseconds of the caller's monotonic clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoffConfig {
    /// A source waits this long from its Offer for an answer and for its
    /// own final snapshot, then aborts and keeps the entity.
    pub answer_ms: u64,
    /// A destination holds a staged snapshot this long without a Commit,
    /// then drops it.
    pub stage_ms: u64,
    /// A source that sent Commit resends it this often until answered.
    pub resend_ms: u64,
    /// Finished handoffs are remembered this long, to answer duplicates.
    /// Longer than the control plane takes to declare a host gone (the
    /// Worker's 90 s heartbeat), so a source still resending a Commit is
    /// always answered from memory.
    pub remember_ms: u64,
}

impl Default for HandoffConfig {
    fn default() -> Self {
        Self { answer_ms: 1_500, stage_ms: 5_000, resend_ms: 250, remember_ms: 120_000 }
    }
}

/// What this host may do with an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authority {
    /// It owns it and simulates it.
    Owned { epoch: u64 },
    /// It offered it to another host, and still owns and simulates it.
    Offering { id: HandoffId, epoch: u64 },
    /// It committed it to another host: it no longer owns it and keeps it
    /// frozen, in custody, until the destination confirms.
    Released { id: HandoffId, epoch: u64 },
}

/// Why a source kept an entity it offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kept {
    Refused(Refusal),
    /// No answer, or no final snapshot, within [`HandoffConfig::answer_ms`].
    Timeout,
    /// The control plane declared the destination gone.
    PeerLost,
    /// The final snapshot broke a rule or did not fit.
    BadSnapshot,
    /// It was destroyed here while being offered.
    Destroyed,
}

/// How a handoff ended, on the side reporting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Source: the destination owns it now. Destination: this host owns it now.
    Moved,
    /// Source: this host still owns it.
    Kept(Kept),
    /// Destination: the staged snapshot was dropped; this host never owned it.
    Dropped,
}

/// What the caller does next.
#[derive(Debug, Clone, PartialEq)]
pub enum HandoffAction {
    /// Send `msg` to host `to` on the shard channel ([`encode_message`]).
    Send { to: Origin, msg: HandoffMsg },
    /// Source, at Offer: tell each rider where it is going, so its Player
    /// asks the Worker for a ticket to `to.cert_sha256` now and holds it.
    PrepareRiders { id: HandoffId, to: Neighbour, riders: Vec<Rider> },
    /// Source: accepted. Take the final snapshot of `root` now and pass it to
    /// [`Handoffs::commit`].
    Finalize { id: HandoffId, root: NetId },
    /// Source: stop simulating `root`; keep it in the world, frozen.
    Freeze { root: NetId },
    /// Source: simulate `root` again; the handoff failed and this host owns it.
    Resume { root: NetId },
    /// Source: the destination owns `root`. Remove it from this world and
    /// send each rider to `to` with its pre-issued ticket.
    Release { id: HandoffId, root: NetId, to: Neighbour, riders: Vec<Rider> },
    /// Destination: make the snapshot ([`materialize`]), put the bodies in
    /// physics, and simulate it from now.
    Spawn { id: HandoffId, snapshot: HandoffSnapshot },
    /// A handoff finished on this side.
    Ended { id: HandoffId, root: NetId, outcome: Outcome },
    /// A message that a correct peer never sends in this state. Logged; no
    /// state changed.
    Anomaly { id: HandoffId, what: &'static str },
}

/// Why [`Handoffs::begin`] did not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeginError {
    /// This host does not own the entity.
    NotHeld,
    /// A handoff of it is already open.
    Busy,
    /// The neighbour is this host, or has no origin.
    BadNeighbour,
    /// The snapshot breaks a rule.
    Snapshot(Refusal),
    /// The Offer would not fit [`MAX_HANDOFF_BYTES`].
    TooLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutPhase {
    Offering,
    Finalizing,
    Released,
    Done(Outcome),
}

#[derive(Debug, Clone)]
struct Outbound {
    root: NetId,
    to: Neighbour,
    epoch: u64,
    phase: OutPhase,
    /// When the Offer went out, or when it ended.
    since: u64,
    /// When the Commit last went out.
    sent: u64,
    riders: Vec<Rider>,
    /// The Commit's snapshot: what a resend carries and what a restore keeps.
    commit: Option<HandoffSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InPhase {
    Staged,
    Held,
    Refused(Refusal),
    Dropped,
}

#[derive(Debug, Clone)]
struct Inbound {
    root: NetId,
    from: Origin,
    epoch: u64,
    phase: InPhase,
    since: u64,
}

/// One host's handoffs, both ways, and what it may do with each entity it
/// knows. One per host process; a Bevy resource while a shard session runs.
#[derive(Resource, Debug)]
pub struct Handoffs {
    me: Origin,
    config: HandoffConfig,
    next_seq: u64,
    authority: HashMap<NetId, Authority>,
    /// Entities that left this host: the epoch their new owner holds them
    /// at, and when they left. Refuses a stale Offer, stops a reclaim, and
    /// answers a late Commit this host already took.
    left: HashMap<NetId, (u64, u64)>,
    outgoing: HashMap<HandoffId, Outbound>,
    incoming: HashMap<HandoffId, Inbound>,
}

impl Handoffs {
    pub fn new(me: Origin, config: HandoffConfig) -> Self {
        Self {
            me,
            config,
            next_seq: 0,
            authority: HashMap::new(),
            left: HashMap::new(),
            outgoing: HashMap::new(),
            incoming: HashMap::new(),
        }
    }

    pub fn me(&self) -> Origin {
        self.me
    }

    pub fn authority(&self, root: NetId) -> Option<Authority> {
        self.authority.get(&root).copied()
    }

    /// This host owns `root` and simulates it.
    pub fn holds(&self, root: NetId) -> bool {
        matches!(self.authority.get(&root), Some(Authority::Owned { .. } | Authority::Offering { .. }))
    }

    /// This host keeps `root` frozen: it committed it and waits for the
    /// destination. Physics skips it.
    pub fn frozen(&self, root: NetId) -> bool {
        matches!(self.authority.get(&root), Some(Authority::Released { .. }))
    }

    /// The newest epoch this host knows `root` at.
    pub fn epoch_of(&self, root: NetId) -> Option<u64> {
        match self.authority.get(&root) {
            Some(Authority::Owned { epoch } | Authority::Offering { epoch, .. } | Authority::Released { epoch, .. }) => {
                Some(*epoch)
            }
            None => self.left.get(&root).map(|(e, _)| *e),
        }
    }

    /// `root` was made on this host: it owns it, at epoch 0. `None` when it
    /// is known to live on another host now (it left from here).
    pub fn claim(&mut self, root: NetId) -> Option<u64> {
        if let Some(epoch) = self.authority.get(&root).map(|a| match a {
            Authority::Owned { epoch } | Authority::Offering { epoch, .. } | Authority::Released { epoch, .. } => *epoch,
        }) {
            return Some(epoch);
        }
        if self.left.contains_key(&root) {
            return None;
        }
        self.authority.insert(root, Authority::Owned { epoch: 0 });
        Some(0)
    }

    /// `root` was destroyed here. An open Offer of it is aborted. A frozen
    /// one is in custody for the destination and stays.
    pub fn forget(&mut self, root: NetId, now: u64) -> Vec<HandoffAction> {
        match self.authority.get(&root).copied() {
            Some(Authority::Owned { .. }) => {
                self.authority.remove(&root);
                vec![]
            }
            Some(Authority::Offering { id, epoch }) => {
                self.authority.remove(&root);
                let Some(out) = self.outgoing.get_mut(&id) else { return vec![] };
                out.phase = OutPhase::Done(Outcome::Kept(Kept::Destroyed));
                out.since = now;
                vec![
                    HandoffAction::Send { to: out.to.origin, msg: HandoffMsg::Abort { id, epoch } },
                    HandoffAction::Ended { id, root, outcome: Outcome::Kept(Kept::Destroyed) },
                ]
            }
            Some(Authority::Released { id, .. }) => {
                vec![HandoffAction::Anomaly { id, what: "destroyed while committed to another host; kept" }]
            }
            None => vec![],
        }
    }

    /// Offer `root`, which this host owns, to `to`. It keeps simulating it.
    pub fn begin(
        &mut self,
        root: NetId,
        to: Neighbour,
        snapshot: HandoffSnapshot,
        now: u64,
    ) -> Result<Vec<HandoffAction>, BeginError> {
        if to.origin == 0 || to.origin == self.me {
            return Err(BeginError::BadNeighbour);
        }
        let epoch = match self.authority.get(&root) {
            Some(Authority::Owned { epoch }) => *epoch,
            Some(_) => return Err(BeginError::Busy),
            None => return Err(BeginError::NotHeld),
        };
        if snapshot.root != root {
            return Err(BeginError::Snapshot(Refusal::Invalid));
        }
        snapshot.check().map_err(BeginError::Snapshot)?;
        self.next_seq += 1;
        let id = HandoffId::new(self.me, self.next_seq);
        let riders = snapshot.riders.clone();
        let msg = HandoffMsg::Offer { id, epoch, snapshot };
        encode_message(&msg).map_err(|_| BeginError::TooLarge)?;
        self.authority.insert(root, Authority::Offering { id, epoch });
        self.outgoing.insert(
            id,
            Outbound { root, to: to.clone(), epoch, phase: OutPhase::Offering, since: now, sent: now, riders: riders.clone(), commit: None },
        );
        let mut actions = vec![HandoffAction::Send { to: to.origin, msg }];
        if !riders.is_empty() {
            actions.push(HandoffAction::PrepareRiders { id, to, riders });
        }
        Ok(actions)
    }

    /// The final snapshot, after [`HandoffAction::Finalize`]: the source
    /// freezes the entity, gives up authority, and sends the Commit. A bad
    /// snapshot aborts instead and the source keeps it.
    pub fn commit(&mut self, id: HandoffId, snapshot: HandoffSnapshot, now: u64) -> Vec<HandoffAction> {
        let Some(out) = self.outgoing.get_mut(&id) else { return vec![] };
        if out.phase != OutPhase::Finalizing {
            return vec![];
        }
        let (root, epoch, to) = (out.root, out.epoch, out.to.origin);
        let msg = HandoffMsg::Commit { id, epoch, snapshot: snapshot.clone() };
        let sendable = snapshot.root == root && snapshot.check().is_ok() && encode_message(&msg).is_ok();
        if !sendable {
            out.phase = OutPhase::Done(Outcome::Kept(Kept::BadSnapshot));
            out.since = now;
            self.authority.insert(root, Authority::Owned { epoch });
            return vec![
                HandoffAction::Send { to, msg: HandoffMsg::Abort { id, epoch } },
                HandoffAction::Ended { id, root, outcome: Outcome::Kept(Kept::BadSnapshot) },
            ];
        }
        out.phase = OutPhase::Released;
        out.sent = now;
        out.riders = snapshot.riders.clone();
        out.commit = Some(snapshot);
        self.authority.insert(root, Authority::Released { id, epoch });
        vec![HandoffAction::Freeze { root }, HandoffAction::Send { to, msg }]
    }

    /// A message from host `from`. `admit` is the destination's own test of a
    /// snapshot (room, free ids, known parents: [`fits`]); it runs on an
    /// Offer and again on the Commit.
    pub fn on_message(
        &mut self,
        from: Origin,
        msg: HandoffMsg,
        now: u64,
        admit: &mut dyn FnMut(&HandoffSnapshot) -> Result<(), Refusal>,
    ) -> Vec<HandoffAction> {
        match msg {
            HandoffMsg::Offer { id, epoch, snapshot } => self.on_offer(from, id, epoch, snapshot, now, admit),
            HandoffMsg::Commit { id, epoch, snapshot } => self.on_commit(from, id, epoch, snapshot, now, admit),
            HandoffMsg::Abort { id, epoch } => self.on_abort(from, id, epoch, now),
            HandoffMsg::Accept { id, epoch } => self.on_accept(from, id, epoch, now),
            HandoffMsg::Refuse { id, epoch, reason } => self.on_refuse(from, id, epoch, reason, now),
            HandoffMsg::Committed { id, epoch } => self.on_committed(from, id, epoch, now),
        }
    }

    // ── Source side ─────────────────────────────────────────────────────────

    /// The open outbound handoff `id`, when `from` and `epoch` are its own.
    fn outbound(&mut self, from: Origin, id: HandoffId, epoch: u64) -> Option<&mut Outbound> {
        self.outgoing.get_mut(&id).filter(|o| o.to.origin == from && o.epoch == epoch)
    }

    fn on_accept(&mut self, from: Origin, id: HandoffId, epoch: u64, now: u64) -> Vec<HandoffAction> {
        let Some(out) = self.outbound(from, id, epoch) else { return vec![] };
        match out.phase {
            OutPhase::Offering => {
                out.phase = OutPhase::Finalizing;
                vec![HandoffAction::Finalize { id, root: out.root }]
            }
            OutPhase::Finalizing | OutPhase::Done(Outcome::Moved) | OutPhase::Done(Outcome::Dropped) => vec![],
            // Its Commit may have been lost: send it again.
            OutPhase::Released => {
                out.sent = now;
                let snapshot = out.commit.clone().expect("a released handoff keeps its commit");
                vec![HandoffAction::Send { to: from, msg: HandoffMsg::Commit { id, epoch, snapshot } }]
            }
            // It aborted; the destination may still be staged.
            OutPhase::Done(Outcome::Kept(_)) => vec![HandoffAction::Send { to: from, msg: HandoffMsg::Abort { id, epoch } }],
        }
    }

    fn on_refuse(&mut self, from: Origin, id: HandoffId, epoch: u64, reason: Refusal, now: u64) -> Vec<HandoffAction> {
        let Some(out) = self.outbound(from, id, epoch) else { return vec![] };
        let root = out.root;
        match out.phase {
            OutPhase::Offering | OutPhase::Finalizing => {
                out.phase = OutPhase::Done(Outcome::Kept(Kept::Refused(reason)));
                out.since = now;
                self.authority.insert(root, Authority::Owned { epoch });
                vec![HandoffAction::Ended { id, root, outcome: Outcome::Kept(Kept::Refused(reason)) }]
            }
            // The destination did not take the Commit: the custody copy
            // comes back to life here.
            OutPhase::Released => {
                out.phase = OutPhase::Done(Outcome::Kept(Kept::Refused(reason)));
                out.since = now;
                self.restore(root, epoch);
                vec![
                    HandoffAction::Resume { root },
                    HandoffAction::Ended { id, root, outcome: Outcome::Kept(Kept::Refused(reason)) },
                ]
            }
            OutPhase::Done(_) => vec![],
        }
    }

    fn on_committed(&mut self, from: Origin, id: HandoffId, epoch: u64, now: u64) -> Vec<HandoffAction> {
        let Some(out) = self.outbound(from, id, epoch) else { return vec![] };
        let root = out.root;
        match out.phase {
            OutPhase::Released => {
                out.phase = OutPhase::Done(Outcome::Moved);
                out.since = now;
                out.commit = None;
                let (to, riders) = (out.to.clone(), std::mem::take(&mut out.riders));
                self.authority.remove(&root);
                self.left.insert(root, (epoch + 1, now));
                vec![
                    HandoffAction::Release { id, root, to, riders },
                    HandoffAction::Ended { id, root, outcome: Outcome::Moved },
                ]
            }
            OutPhase::Done(Outcome::Moved) | OutPhase::Done(Outcome::Dropped) => vec![],
            OutPhase::Done(Outcome::Kept(_)) => {
                vec![HandoffAction::Anomaly { id, what: "committed after the source took it back" }]
            }
            OutPhase::Offering | OutPhase::Finalizing => {
                vec![HandoffAction::Anomaly { id, what: "committed before any commit was sent" }]
            }
        }
    }

    /// The source owns `root` again after it released it. The epoch moves
    /// past anything the destination could have held it at.
    fn restore(&mut self, root: NetId, epoch: u64) {
        self.authority.insert(root, Authority::Owned { epoch: epoch + 2 });
    }

    // ── Destination side ────────────────────────────────────────────────────

    fn reply(to: Origin, msg: HandoffMsg) -> Vec<HandoffAction> {
        vec![HandoffAction::Send { to, msg }]
    }

    /// Answer `id` from what this side already decided.
    fn answer(&self, rec: &Inbound, id: HandoffId) -> Vec<HandoffAction> {
        let epoch = rec.epoch;
        let msg = match rec.phase {
            InPhase::Staged => HandoffMsg::Accept { id, epoch },
            InPhase::Held => HandoffMsg::Committed { id, epoch },
            InPhase::Refused(reason) => HandoffMsg::Refuse { id, epoch, reason },
            InPhase::Dropped => HandoffMsg::Refuse { id, epoch, reason: Refusal::NotHeld },
        };
        Self::reply(rec.from, msg)
    }

    fn on_offer(
        &mut self,
        from: Origin,
        id: HandoffId,
        epoch: u64,
        snapshot: HandoffSnapshot,
        now: u64,
        admit: &mut dyn FnMut(&HandoffSnapshot) -> Result<(), Refusal>,
    ) -> Vec<HandoffAction> {
        if let Some(rec) = self.incoming.get(&id) {
            if rec.from != from || rec.epoch != epoch {
                return vec![];
            }
            // A duplicate. Held: the Committed already went, and stays the answer.
            return match rec.phase {
                InPhase::Held => vec![],
                _ => self.answer(rec, id),
            };
        }
        let root = snapshot.root;
        // An id names the host that began it; a host cannot offer for another.
        if id.origin() != from || from == self.me {
            return Self::reply(from, HandoffMsg::Refuse { id, epoch, reason: Refusal::Invalid });
        }
        let verdict = snapshot
            .check()
            .and_then(|()| if self.authority.contains_key(&root) { Err(Refusal::AlreadyHeld) } else { Ok(()) })
            .and_then(|()| {
                let open = self.incoming.values().any(|r| r.root == root && r.phase == InPhase::Staged);
                if open { Err(Refusal::Busy) } else { Ok(()) }
            })
            .and_then(|()| match self.epoch_of(root) {
                Some(known) if epoch < known => Err(Refusal::Stale),
                _ => Ok(()),
            })
            .and_then(|()| admit(&snapshot));
        let phase = match verdict {
            Ok(()) => InPhase::Staged,
            Err(reason) => InPhase::Refused(reason),
        };
        let rec = Inbound { root, from, epoch, phase, since: now };
        let answer = self.answer(&rec, id);
        self.incoming.insert(id, rec);
        answer
    }

    fn on_commit(
        &mut self,
        from: Origin,
        id: HandoffId,
        epoch: u64,
        snapshot: HandoffSnapshot,
        now: u64,
        admit: &mut dyn FnMut(&HandoffSnapshot) -> Result<(), Refusal>,
    ) -> Vec<HandoffAction> {
        if id.origin() != from {
            return vec![];
        }
        let root = snapshot.root;
        let Some(rec) = self.incoming.get_mut(&id) else {
            // No record: never offered here, or remembered no longer. Only
            // this host can have taken it at epoch + 1 (the Commit names it),
            // so knowing that epoch or a newer one means it did.
            if self.epoch_of(root).is_some_and(|known| known > epoch) {
                return Self::reply(from, HandoffMsg::Committed { id, epoch });
            }
            // A later Offer under this id is refused too.
            self.incoming.insert(id, Inbound { root, from, epoch, phase: InPhase::Dropped, since: now });
            return Self::reply(from, HandoffMsg::Refuse { id, epoch, reason: Refusal::NotHeld });
        };
        if rec.from != from || rec.epoch != epoch {
            return vec![];
        }
        if rec.phase != InPhase::Staged {
            // A duplicate Commit is answered, never applied again.
            let rec = rec.clone();
            return self.answer(&rec, id);
        }
        let verdict = if snapshot.root != rec.root {
            Err(Refusal::Invalid)
        } else if self.authority.contains_key(&root) {
            Err(Refusal::AlreadyHeld)
        } else {
            snapshot.check().and_then(|()| admit(&snapshot))
        };
        let rec = self.incoming.get_mut(&id).expect("looked up above");
        rec.since = now;
        match verdict {
            Err(reason) => {
                rec.phase = InPhase::Refused(reason);
                vec![
                    HandoffAction::Send { to: from, msg: HandoffMsg::Refuse { id, epoch, reason } },
                    HandoffAction::Ended { id, root, outcome: Outcome::Dropped },
                ]
            }
            Ok(()) => {
                // The ownership flip: this host owns it from this message on.
                rec.phase = InPhase::Held;
                self.authority.insert(root, Authority::Owned { epoch: epoch + 1 });
                self.left.remove(&root);
                vec![
                    HandoffAction::Spawn { id, snapshot },
                    HandoffAction::Send { to: from, msg: HandoffMsg::Committed { id, epoch } },
                    HandoffAction::Ended { id, root, outcome: Outcome::Moved },
                ]
            }
        }
    }

    fn on_abort(&mut self, from: Origin, id: HandoffId, epoch: u64, now: u64) -> Vec<HandoffAction> {
        if id.origin() != from {
            return vec![];
        }
        let Some(rec) = self.incoming.get_mut(&id) else {
            // Before its Offer: remember it, so that Offer is refused.
            self.incoming.insert(id, Inbound { root: NetId::NONE, from, epoch, phase: InPhase::Dropped, since: now });
            return vec![];
        };
        if rec.from != from || rec.epoch != epoch {
            return vec![];
        }
        match rec.phase {
            InPhase::Staged => {
                rec.phase = InPhase::Dropped;
                rec.since = now;
                vec![HandoffAction::Ended { id, root: rec.root, outcome: Outcome::Dropped }]
            }
            InPhase::Held => vec![HandoffAction::Anomaly { id, what: "abort after commit; this host keeps it" }],
            InPhase::Refused(_) | InPhase::Dropped => vec![],
        }
    }

    // ── Time and the control plane ──────────────────────────────────────────

    /// Timers: an unanswered Offer aborts (the source keeps the entity), a
    /// Commit not yet confirmed is sent again (never resumed on a timer), a
    /// staged snapshot left without a Commit is dropped, and finished
    /// handoffs are forgotten after [`HandoffConfig::remember_ms`].
    pub fn tick(&mut self, now: u64) -> Vec<HandoffAction> {
        let c = self.config;
        let mut actions = Vec::new();
        let mut ids: Vec<HandoffId> = self.outgoing.keys().copied().collect();
        ids.sort();
        for id in ids {
            let out = self.outgoing.get_mut(&id).expect("listed above");
            let (root, epoch, to) = (out.root, out.epoch, out.to.origin);
            match out.phase {
                OutPhase::Offering | OutPhase::Finalizing if now.saturating_sub(out.since) >= c.answer_ms => {
                    out.phase = OutPhase::Done(Outcome::Kept(Kept::Timeout));
                    out.since = now;
                    self.authority.insert(root, Authority::Owned { epoch });
                    actions.push(HandoffAction::Send { to, msg: HandoffMsg::Abort { id, epoch } });
                    actions.push(HandoffAction::Ended { id, root, outcome: Outcome::Kept(Kept::Timeout) });
                }
                OutPhase::Released if now.saturating_sub(out.sent) >= c.resend_ms => {
                    out.sent = now;
                    let snapshot = out.commit.clone().expect("a released handoff keeps its commit");
                    actions.push(HandoffAction::Send { to, msg: HandoffMsg::Commit { id, epoch, snapshot } });
                }
                OutPhase::Done(_) if now.saturating_sub(out.since) >= c.remember_ms => {
                    self.outgoing.remove(&id);
                }
                _ => {}
            }
        }
        let mut ids: Vec<HandoffId> = self.incoming.keys().copied().collect();
        ids.sort();
        for id in ids {
            let rec = self.incoming.get_mut(&id).expect("listed above");
            match rec.phase {
                InPhase::Staged if now.saturating_sub(rec.since) >= c.stage_ms => {
                    rec.phase = InPhase::Dropped;
                    rec.since = now;
                    actions.push(HandoffAction::Ended { id, root: rec.root, outcome: Outcome::Dropped });
                }
                InPhase::Held | InPhase::Refused(_) | InPhase::Dropped if now.saturating_sub(rec.since) >= c.remember_ms => {
                    self.incoming.remove(&id);
                }
                _ => {}
            }
        }
        self.left.retain(|_, (_, at)| now.saturating_sub(*at) < c.remember_ms);
        actions
    }

    /// The control plane declared host `origin` gone (its live heartbeat
    /// lapsed, and its credential with it). Of this host's handoffs to it, an
    /// open Offer ends with this host keeping the entity, and a Commit it
    /// never confirmed brings the frozen copy back to life here. Of its
    /// handoffs to this host, a staged snapshot is dropped, and an entity it
    /// committed here stays here.
    pub fn peer_lost(&mut self, origin: Origin, now: u64) -> Vec<HandoffAction> {
        let mut actions = Vec::new();
        let mut ids: Vec<HandoffId> = self.outgoing.iter().filter(|(_, o)| o.to.origin == origin).map(|(id, _)| *id).collect();
        ids.sort();
        for id in ids {
            let out = self.outgoing.get_mut(&id).expect("listed above");
            let (root, epoch) = (out.root, out.epoch);
            match out.phase {
                OutPhase::Offering | OutPhase::Finalizing => {
                    out.phase = OutPhase::Done(Outcome::Kept(Kept::PeerLost));
                    out.since = now;
                    self.authority.insert(root, Authority::Owned { epoch });
                    actions.push(HandoffAction::Ended { id, root, outcome: Outcome::Kept(Kept::PeerLost) });
                }
                OutPhase::Released => {
                    out.phase = OutPhase::Done(Outcome::Kept(Kept::PeerLost));
                    out.since = now;
                    self.restore(root, epoch);
                    actions.push(HandoffAction::Resume { root });
                    actions.push(HandoffAction::Ended { id, root, outcome: Outcome::Kept(Kept::PeerLost) });
                }
                OutPhase::Done(_) => {}
            }
        }
        let mut ids: Vec<HandoffId> = self.incoming.iter().filter(|(_, r)| r.from == origin).map(|(id, _)| *id).collect();
        ids.sort();
        for id in ids {
            let rec = self.incoming.get_mut(&id).expect("listed above");
            if rec.phase == InPhase::Staged {
                rec.phase = InPhase::Dropped;
                rec.since = now;
                actions.push(HandoffAction::Ended { id, root: rec.root, outcome: Outcome::Dropped });
            }
        }
        actions
    }
}

// ── The replication side: ids that stay the same across hosts ───────────────

/// The snapshot of `root` and of each of `also` (riders' characters) as this
/// host's players have them, at `tick`. Bodies, owner and riders are the
/// caller's to fill from physics and seats. `None` when players do not have
/// `root`.
pub fn snapshot_of(host: &HostReplicator, dm: &DataModel, root: InstanceId, also: &[InstanceId], tick: u64) -> Option<HandoffSnapshot> {
    let mut instances = host.handoff_ops(dm, root)?;
    let root_net = instances.first()?.id;
    let mut have: HashSet<NetId> = instances.iter().map(|op| op.id).collect();
    for extra in also {
        // A character already inside the entity (sitting in it) comes once.
        for op in host.handoff_ops(dm, *extra).unwrap_or_default() {
            if have.insert(op.id) {
                instances.push(op);
            }
        }
    }
    Some(HandoffSnapshot { root: root_net, tick, instances, bodies: Vec::new(), owner: None, riders: Vec::new() })
}

/// The destination's test of a snapshot against its own world: well formed,
/// every id free here, and every parent it hangs under present here. Room
/// (capacity) is the caller's to add.
pub fn fits(host: &HostReplicator, dm: &DataModel, snapshot: &HandoffSnapshot) -> Result<(), Refusal> {
    snapshot.check()?;
    let ids: HashSet<NetId> = snapshot.ids().collect();
    for s in &snapshot.instances {
        if !host.id_free(s.id) {
            return Err(Refusal::IdTaken);
        }
        if !ids.contains(&s.parent) && !host.local_of(s.parent).is_some_and(|p| dm.exists(p)) {
            return Err(Refusal::UnknownParent);
        }
    }
    Ok(())
}

/// Make the snapshot's instances in `dm` through the same path a player
/// applies a world frame, and give each the id it had on the source, so this
/// host's players are sent it under that id. Returns each id with the
/// instance made for it. A reference to an instance outside the snapshot and
/// outside this world arrives as nil, as it does on a player.
pub fn materialize(
    dm: &mut DataModel,
    host: &mut HostReplicator,
    snapshot: &HandoffSnapshot,
) -> Result<Vec<(NetId, InstanceId)>, Refusal> {
    fits(host, dm, snapshot)?;
    let ids: HashSet<NetId> = snapshot.ids().collect();
    let mut replica = Replica::new(dm);
    for s in &snapshot.instances {
        if !ids.contains(&s.parent) {
            let parent = host.local_of(s.parent).ok_or(Refusal::UnknownParent)?;
            replica.bind_net(s.parent, parent);
        }
    }
    let frame = WorldFrame { tick: snapshot.tick, ops: snapshot.instances.iter().cloned().map(ReplOp::Spawn).collect() };
    let _ = replica.apply(dm, &frame);
    let mut made = Vec::with_capacity(snapshot.instances.len());
    for s in &snapshot.instances {
        let local = replica.local_of(s.id).ok_or(Refusal::Invalid)?;
        host.adopt(s.id, local).map_err(|_| Refusal::IdTaken)?;
        made.push((s.id, local));
    }
    Ok(made)
}

#[cfg(test)]
mod tests {
    use eustress_common::datamodel::DmValue;
    use eustress_common::scripting::{CFrame, Vector3};

    use super::*;
    use crate::repl::{service_net_id, FrameEffects, Outgoing, WireValue};

    const A: Origin = 1;
    const B: Origin = 2;

    fn neighbour(origin: Origin) -> Neighbour {
        Neighbour { origin, server_id: format!("s{origin}"), link: format!("eustress-player://join/h{origin}"), cert_sha256: "ab".repeat(32) }
    }

    fn spawn(id: NetId, class: &str, name: &str, parent: NetId) -> SpawnOp {
        SpawnOp { id, class: class.into(), name: name.into(), parent, props: vec![], attributes: vec![], tags: vec![], peer: None }
    }

    fn body(id: NetId, x: f32) -> HandoffBody {
        HandoffBody {
            id,
            position: [x, 2.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            linear: [20.0, 0.0, 0.0],
            angular: [0.0, 0.5, 0.0],
            kind: BodyKind::Dynamic,
            sleeping: false,
        }
    }

    /// A car with a seat and a rider's character, minted by host A.
    fn car(x: f32) -> HandoffSnapshot {
        let ws = service_net_id("Workspace");
        let root = NetId::runtime_in(A, 10);
        let seat = NetId::runtime_in(A, 11);
        let character = NetId::runtime_in(A, 12);
        let mut seat_op = spawn(seat, "Part", "Seat", root);
        seat_op.props.push(("Anchored".into(), WireValue::Bool(false)));
        let mut root_op = spawn(root, "Model", "Car", ws);
        root_op.attributes.push(("Fuel".into(), WireValue::Number(0.75)));
        HandoffSnapshot {
            root,
            tick: 100,
            instances: vec![root_op, seat_op, spawn(character, "Model", "Rider", ws)],
            bodies: vec![body(seat, x)],
            owner: Some("acct_owner".into()),
            riders: vec![Rider { account: "acct_1".into(), player: NetId::runtime_in(A, 5), character: Some(character), seat: Some(seat) }],
        }
    }

    fn ok(_: &HandoffSnapshot) -> Result<(), Refusal> {
        Ok(())
    }

    fn sent(actions: &[HandoffAction]) -> Vec<HandoffMsg> {
        actions
            .iter()
            .filter_map(|a| match a {
                HandoffAction::Send { msg, .. } => Some(msg.clone()),
                _ => None,
            })
            .collect()
    }

    /// Through the wire codec, as the shard channel carries it.
    fn wire(msg: &HandoffMsg) -> HandoffMsg {
        let frame = encode_message(msg).expect("encodes");
        decode_message(&frame[4..]).expect("decodes")
    }

    fn deliver(to: &mut Handoffs, from: Origin, msgs: &[HandoffMsg], now: u64) -> Vec<HandoffAction> {
        msgs.iter().flat_map(|m| to.on_message(from, wire(m), now, &mut ok)).collect()
    }

    fn has(actions: &[HandoffAction], pred: impl Fn(&HandoffAction) -> bool) -> bool {
        actions.iter().any(pred)
    }

    /// Never two hosts simulating it, and never nobody holding it unless the
    /// source keeps it frozen in custody.
    fn one_owner(a: &Handoffs, b: &Handoffs, root: NetId) {
        let holders = [a.holds(root), b.holds(root)].into_iter().filter(|h| *h).count();
        assert!(holders <= 1, "two hosts own {root:?}: {:?} and {:?}", a.authority(root), b.authority(root));
        if holders == 0 {
            assert!(a.frozen(root) || b.frozen(root), "nobody owns {root:?} and nobody keeps it");
        }
    }

    /// A owns the car and has offered it to B; B staged it.
    fn offered() -> (Handoffs, Handoffs, NetId, HandoffId, Vec<HandoffAction>) {
        let mut a = Handoffs::new(A, HandoffConfig::default());
        let mut b = Handoffs::new(B, HandoffConfig::default());
        let snap = car(0.0);
        let root = snap.root;
        assert_eq!(a.claim(root), Some(0));
        let begun = a.begin(root, neighbour(B), snap, 0).expect("begins");
        let id = match &sent(&begun)[0] {
            HandoffMsg::Offer { id, .. } => *id,
            other => panic!("first message {other:?}"),
        };
        let answer = deliver(&mut b, A, &sent(&begun), 10);
        assert_eq!(sent(&answer), vec![HandoffMsg::Accept { id, epoch: 0 }]);
        one_owner(&a, &b, root);
        (a, b, root, id, answer)
    }

    /// From the Offer on: A finalizes, commits, B takes it, A releases.
    #[test]
    fn ownership_flips_only_on_commit() {
        let (mut a, mut b, root, id, accept) = offered();
        // Offered and accepted: A still owns and simulates it; B only staged it.
        assert!(a.holds(root) && !a.frozen(root));
        assert!(!b.holds(root), "the destination owned it before any commit");

        let finalize = deliver(&mut a, B, &sent(&accept), 20);
        assert_eq!(finalize, vec![HandoffAction::Finalize { id, root }]);
        assert!(a.holds(root), "accepting is not committing");
        one_owner(&a, &b, root);

        let commit = a.commit(id, car(4.0), 21);
        assert_eq!(commit[0], HandoffAction::Freeze { root });
        assert!(!a.holds(root) && a.frozen(root), "the source still simulates what it committed");
        assert!(!b.holds(root));
        one_owner(&a, &b, root);

        let taken = deliver(&mut b, A, &sent(&commit), 30);
        assert!(b.holds(root), "the destination does not own what was committed to it");
        assert_eq!(b.epoch_of(root), Some(1));
        let spawned: Vec<_> = taken.iter().filter(|x| matches!(x, HandoffAction::Spawn { .. })).collect();
        assert_eq!(spawned.len(), 1);
        if let HandoffAction::Spawn { snapshot, .. } = spawned[0] {
            assert_eq!(snapshot.bodies[0].position[0], 4.0, "the destination spawned the offer, not the final snapshot");
        }
        one_owner(&a, &b, root);

        let released = deliver(&mut a, B, &sent(&taken), 40);
        assert!(has(&released, |x| matches!(x, HandoffAction::Release { riders, .. } if riders.len() == 1)));
        assert!(has(&released, |x| *x == HandoffAction::Ended { id, root, outcome: Outcome::Moved }));
        assert_eq!(a.authority(root), None);
        assert!(!a.frozen(root));
        one_owner(&a, &b, root);
        assert_eq!(a.claim(root), None, "the source reclaimed what it handed away");
    }

    #[test]
    fn riders_are_prepared_at_the_offer_with_the_neighbours_pin() {
        let mut a = Handoffs::new(A, HandoffConfig::default());
        let snap = car(0.0);
        a.claim(snap.root);
        let begun = a.begin(snap.root, neighbour(B), snap.clone(), 0).unwrap();
        assert!(has(&begun, |x| matches!(x, HandoffAction::PrepareRiders { to, riders, .. }
            if to.cert_sha256 == neighbour(B).cert_sha256 && riders == &snap.riders)));
    }

    #[test]
    fn a_refusal_leaves_the_source_owning_it() {
        let mut a = Handoffs::new(A, HandoffConfig::default());
        let mut b = Handoffs::new(B, HandoffConfig::default());
        let snap = car(0.0);
        let root = snap.root;
        a.claim(root);
        let begun = a.begin(root, neighbour(B), snap, 0).unwrap();
        let answer: Vec<_> = sent(&begun).iter().flat_map(|m| b.on_message(A, wire(m), 5, &mut |_| Err(Refusal::Full))).collect();
        assert!(matches!(sent(&answer)[0], HandoffMsg::Refuse { reason: Refusal::Full, .. }));
        let back = deliver(&mut a, B, &sent(&answer), 10);
        assert!(has(&back, |x| matches!(x, HandoffAction::Ended { outcome: Outcome::Kept(Kept::Refused(Refusal::Full)), .. })));
        assert_eq!(a.authority(root), Some(Authority::Owned { epoch: 0 }));
        assert!(!b.holds(root));
        // It can be offered again.
        assert!(a.begin(root, neighbour(B), car(1.0), 20).is_ok());
    }

    /// The destination dies before it takes the Commit, in both places it
    /// can: before it answers, and after it accepted.
    #[test]
    fn a_destination_crash_before_commit_leaves_the_source_owning_it() {
        // 1. Silent: the source aborts on its timer and never stopped simulating.
        let mut a = Handoffs::new(A, HandoffConfig::default());
        let snap = car(0.0);
        let root = snap.root;
        a.claim(root);
        let begun = a.begin(root, neighbour(B), snap, 0).unwrap();
        let id = sent(&begun)[0].id();
        assert!(a.tick(1_000).is_empty(), "aborted before its time");
        assert!(a.holds(root));
        let timed_out = a.tick(1_500);
        assert_eq!(sent(&timed_out), vec![HandoffMsg::Abort { id, epoch: 0 }]);
        assert_eq!(a.authority(root), Some(Authority::Owned { epoch: 0 }));

        // 2. Accepted, committed, and the destination dies with the Commit.
        let (mut a, b, root, id, accept) = offered();
        deliver(&mut a, B, &sent(&accept), 20);
        let commit = a.commit(id, car(4.0), 21);
        drop((b, commit));
        // Unconfirmed: the source resends, and never resumes on a timer.
        for t in [300, 1_000, 10_000, 100_000] {
            let again = a.tick(t);
            assert!(sent(&again).iter().all(|m| matches!(m, HandoffMsg::Commit { .. })));
            assert!(a.frozen(root), "the source resumed on a timer at {t} ms");
        }
        let lost = a.peer_lost(B, 100_100);
        assert!(has(&lost, |x| *x == HandoffAction::Resume { root }));
        assert!(a.holds(root));
        assert_eq!(a.authority(root), Some(Authority::Owned { epoch: 2 }), "the epoch must pass the destination's");
    }

    #[test]
    fn a_source_crash_after_commit_leaves_the_destination_owning_it() {
        let (mut a, mut b, root, id, accept) = offered();
        deliver(&mut a, B, &sent(&accept), 20);
        let commit = a.commit(id, car(4.0), 21);
        deliver(&mut b, A, &sent(&commit), 30);
        drop(a); // The source dies before it hears Committed.
        assert!(b.peer_lost(A, 40).is_empty());
        assert!(b.holds(root));
        b.tick(1_000_000);
        assert!(b.holds(root), "the destination gave up what it owns");
        assert_eq!(b.authority(root), Some(Authority::Owned { epoch: 1 }));
    }

    /// A source that dies before its Commit takes the entity with it: the
    /// destination never owned it and drops the staged copy.
    #[test]
    fn a_source_crash_before_commit_drops_the_staged_copy() {
        let (_a, mut b, root, id, _) = offered();
        let dropped = b.peer_lost(A, 50);
        assert_eq!(dropped, vec![HandoffAction::Ended { id, root, outcome: Outcome::Dropped }]);
        assert!(!b.holds(root));

        // On the timer, too, and a Commit that arrives after is refused.
        let (mut a, mut b, root, id, accept) = offered();
        assert!(has(&b.tick(5_010), |x| matches!(x, HandoffAction::Ended { outcome: Outcome::Dropped, .. })));
        deliver(&mut a, B, &sent(&accept), 5_020);
        let commit = a.commit(id, car(4.0), 5_021);
        let late = deliver(&mut b, A, &sent(&commit), 5_030);
        assert_eq!(sent(&late), vec![HandoffMsg::Refuse { id, epoch: 0, reason: Refusal::NotHeld }]);
        assert!(!b.holds(root));
        let back = deliver(&mut a, B, &sent(&late), 5_040);
        assert!(has(&back, |x| *x == HandoffAction::Resume { root }));
        assert!(a.holds(root));
        one_owner(&a, &b, root);
    }

    #[test]
    fn a_duplicate_commit_is_answered_and_never_applied_twice() {
        let (mut a, mut b, root, id, accept) = offered();
        deliver(&mut a, B, &sent(&accept), 20);
        let commit = a.commit(id, car(4.0), 21);
        let first = deliver(&mut b, A, &sent(&commit), 30);
        assert!(has(&first, |x| matches!(x, HandoffAction::Spawn { .. })));
        // B moves the car on; then the Commit arrives again (A resent it).
        let second = deliver(&mut b, A, &sent(&commit), 400);
        assert_eq!(second, vec![HandoffAction::Send { to: A, msg: HandoffMsg::Committed { id, epoch: 0 } }]);
        assert_eq!(b.authority(root), Some(Authority::Owned { epoch: 1 }));

        // And the source hears Committed twice: one release.
        let mut released = deliver(&mut a, B, &sent(&first), 40);
        released.extend(deliver(&mut a, B, &sent(&second), 410));
        let releases = released.iter().filter(|x| matches!(x, HandoffAction::Release { .. })).count();
        assert_eq!(releases, 1, "the source released twice");
        one_owner(&a, &b, root);
    }

    /// A Commit resent after the destination forgot the handoff is still
    /// answered Committed: forgetting must never undo the flip.
    #[test]
    fn a_commit_remembered_no_longer_is_still_answered_committed() {
        let (mut a, mut b, root, id, accept) = offered();
        deliver(&mut a, B, &sent(&accept), 20);
        let commit = a.commit(id, car(4.0), 21);
        deliver(&mut b, A, &sent(&commit), 30);
        b.tick(500_000);
        let answer = deliver(&mut b, A, &sent(&commit), 500_001);
        assert_eq!(sent(&answer), vec![HandoffMsg::Committed { id, epoch: 0 }]);
        assert!(!has(&answer, |x| matches!(x, HandoffAction::Spawn { .. })));
        assert!(b.holds(root));
    }

    #[test]
    fn duplicate_and_late_messages_are_idempotent() {
        let (mut a, mut b, root, id, accept) = offered();
        // A duplicate Offer gets the same Accept and stages nothing new.
        let offer = HandoffMsg::Offer { id, epoch: 0, snapshot: car(0.0) };
        assert_eq!(sent(&deliver(&mut b, A, &[offer.clone()], 15)), vec![HandoffMsg::Accept { id, epoch: 0 }]);
        // Committed before any Commit: refused as an anomaly, nothing moves.
        let early = deliver(&mut a, B, &[HandoffMsg::Committed { id, epoch: 0 }], 16);
        assert!(matches!(early.as_slice(), [HandoffAction::Anomaly { .. }]));
        assert!(a.holds(root));
        // Accept twice: one Finalize.
        let mut f = deliver(&mut a, B, &sent(&accept), 20);
        f.extend(deliver(&mut a, B, &sent(&accept), 21));
        assert_eq!(f, vec![HandoffAction::Finalize { id, root }]);
        // After the Commit, a late Accept resends the Commit (it may be lost).
        let commit = a.commit(id, car(4.0), 22);
        assert!(a.commit(id, car(5.0), 23).is_empty(), "a second commit call was taken");
        let again = deliver(&mut a, B, &sent(&accept), 24);
        assert_eq!(sent(&again), sent(&commit));
        // Duplicate Offer after B holds it: no answer, nothing staged.
        deliver(&mut b, A, &sent(&commit), 30);
        assert!(deliver(&mut b, A, &[offer], 31).is_empty());
        one_owner(&a, &b, root);

        // An Accept that arrives after the source gave up gets the Abort again.
        let mut a = Handoffs::new(A, HandoffConfig::default());
        let mut b = Handoffs::new(B, HandoffConfig::default());
        let snap = car(0.0);
        a.claim(snap.root);
        let begun = a.begin(snap.root, neighbour(B), snap.clone(), 0).unwrap();
        let id = sent(&begun)[0].id();
        a.tick(2_000);
        let late_accept = deliver(&mut b, A, &sent(&begun), 2_001);
        let answer = deliver(&mut a, B, &sent(&late_accept), 2_002);
        assert_eq!(sent(&answer), vec![HandoffMsg::Abort { id, epoch: 0 }]);
        let dropped = deliver(&mut b, A, &sent(&answer), 2_003);
        assert!(has(&dropped, |x| matches!(x, HandoffAction::Ended { outcome: Outcome::Dropped, .. })));
        assert!(a.holds(snap.root) && !b.holds(snap.root));
    }

    #[test]
    fn out_of_order_and_foreign_messages_change_nothing() {
        // Abort before its Offer: the Offer is then refused.
        let mut b = Handoffs::new(B, HandoffConfig::default());
        let id = HandoffId::new(A, 1);
        assert!(deliver(&mut b, A, &[HandoffMsg::Abort { id, epoch: 0 }], 0).is_empty());
        let answer = deliver(&mut b, A, &[HandoffMsg::Offer { id, epoch: 0, snapshot: car(0.0) }], 1);
        assert_eq!(sent(&answer), vec![HandoffMsg::Refuse { id, epoch: 0, reason: Refusal::NotHeld }]);
        assert!(!b.holds(car(0.0).root));

        // A Commit with no Offer before it is refused, and owns nothing.
        let mut b = Handoffs::new(B, HandoffConfig::default());
        let answer = deliver(&mut b, A, &[HandoffMsg::Commit { id, epoch: 0, snapshot: car(0.0) }], 0);
        assert_eq!(sent(&answer), vec![HandoffMsg::Refuse { id, epoch: 0, reason: Refusal::NotHeld }]);
        assert!(!b.holds(car(0.0).root));

        // Messages under another epoch, or from a host the handoff is not
        // with, are ignored on both sides.
        let (mut a, mut b, root, id, _) = offered();
        let third: Origin = 3;
        assert!(deliver(&mut a, third, &[HandoffMsg::Accept { id, epoch: 0 }], 20).is_empty());
        assert!(deliver(&mut a, B, &[HandoffMsg::Accept { id, epoch: 7 }], 20).is_empty());
        assert!(deliver(&mut b, third, &[HandoffMsg::Commit { id, epoch: 0, snapshot: car(0.0) }], 20).is_empty());
        assert!(deliver(&mut b, A, &[HandoffMsg::Commit { id, epoch: 7, snapshot: car(0.0) }], 20).is_empty());
        assert!(deliver(&mut b, A, &[HandoffMsg::Abort { id, epoch: 7 }], 20).is_empty());
        assert!(a.holds(root) && !b.holds(root));
        // A host may not offer under another host's id.
        let forged = deliver(&mut b, third, &[HandoffMsg::Offer { id: HandoffId::new(A, 99), epoch: 0, snapshot: car(0.0) }], 21);
        assert!(matches!(sent(&forged).as_slice(), [HandoffMsg::Refuse { reason: Refusal::Invalid, .. }]));
    }

    /// There and back: A to B (epoch 0 to 1), then B to A (1 to 2). The first
    /// Offer replayed after all that is stale everywhere.
    #[test]
    fn a_stale_epoch_is_refused() {
        let (mut a, mut b, root, id, accept) = offered();
        deliver(&mut a, B, &sent(&accept), 20);
        let commit = a.commit(id, car(4.0), 21);
        let taken = deliver(&mut b, A, &sent(&commit), 30);
        deliver(&mut a, B, &sent(&taken), 40);

        let mut back = car(9.0);
        back.root = root;
        let begun = b.begin(root, neighbour(A), back.clone(), 50).expect("B offers it back");
        let answer = deliver(&mut a, B, &sent(&begun), 60);
        let back_id = sent(&begun)[0].id();
        assert_eq!(sent(&answer), vec![HandoffMsg::Accept { id: back_id, epoch: 1 }]);
        deliver(&mut b, A, &sent(&answer), 70);
        let commit_back = b.commit(back_id, back, 71);
        let taken_back = deliver(&mut a, B, &sent(&commit_back), 80);
        deliver(&mut b, A, &sent(&taken_back), 90);
        assert_eq!(a.authority(root), Some(Authority::Owned { epoch: 2 }));
        assert_eq!(b.authority(root), None);

        // The very first Offer, replayed under a new id: B has seen epoch 2.
        let replay = HandoffMsg::Offer { id: HandoffId::new(A, 50), epoch: 0, snapshot: car(0.0) };
        let refused = deliver(&mut b, A, &[replay], 100);
        assert!(matches!(sent(&refused).as_slice(), [HandoffMsg::Refuse { reason: Refusal::Stale, .. }]));
        // And an Offer of something B owns is refused as held.
        let (mut a2, mut b2, root2, ..) = offered();
        let _ = (&mut a2, root2);
        let dup = HandoffMsg::Offer { id: HandoffId::new(A, 77), epoch: 0, snapshot: car(0.0) };
        let busy = deliver(&mut b2, A, &[dup], 11);
        assert!(matches!(sent(&busy).as_slice(), [HandoffMsg::Refuse { reason: Refusal::Busy, .. }]));
    }

    #[test]
    fn a_bad_final_snapshot_aborts_and_the_source_keeps_it() {
        let (mut a, mut b, root, id, accept) = offered();
        deliver(&mut a, B, &sent(&accept), 20);
        let mut bad = car(4.0);
        bad.bodies[0].linear[0] = f32::NAN;
        let actions = a.commit(id, bad, 21);
        assert_eq!(sent(&actions), vec![HandoffMsg::Abort { id, epoch: 0 }]);
        assert!(a.holds(root) && !a.frozen(root));
        deliver(&mut b, A, &sent(&actions), 22);
        one_owner(&a, &b, root);
        assert!(!b.holds(root));
    }

    #[test]
    fn snapshots_are_bounded_and_well_formed() {
        assert_eq!(car(0.0).check(), Ok(()));
        let broken = |f: &dyn Fn(&mut HandoffSnapshot)| {
            let mut s = car(0.0);
            f(&mut s);
            s.check()
        };
        assert_eq!(broken(&|s| s.instances.clear()), Err(Refusal::Invalid));
        assert_eq!(broken(&|s| s.instances.swap(0, 1)), Err(Refusal::Invalid), "the root must come first");
        assert_eq!(broken(&|s| s.instances[2].id = s.instances[1].id), Err(Refusal::Invalid), "duplicate ids");
        assert_eq!(broken(&|s| s.bodies[0].position[1] = f32::INFINITY), Err(Refusal::Invalid));
        assert_eq!(broken(&|s| s.bodies[0].rotation = [0.0, 0.0, 0.0, 2.0]), Err(Refusal::Invalid));
        assert_eq!(broken(&|s| s.bodies[0].id = NetId::runtime_in(A, 999)), Err(Refusal::Invalid));
        assert_eq!(broken(&|s| s.riders[0].seat = Some(NetId::runtime_in(A, 999))), Err(Refusal::Invalid));
        assert_eq!(broken(&|s| s.riders[0].account = "has space".into()), Err(Refusal::Invalid));
        assert_eq!(broken(&|s| s.instances[1].props.push(("CFrame".into(), WireValue::Vector3([f64::NAN, 0.0, 0.0])))), Err(Refusal::Invalid));
        // A child before its parent.
        assert_eq!(
            broken(&|s| {
                let child = spawn(NetId::runtime_in(A, 40), "Part", "Wheel", NetId::runtime_in(A, 41));
                s.instances.push(child);
                s.instances.push(spawn(NetId::runtime_in(A, 41), "Model", "Axle", s.root));
            }),
            Err(Refusal::Invalid)
        );
        assert_eq!(
            broken(&|s| {
                let extra: Vec<_> = (0..MAX_HANDOFF_INSTANCES as u64).map(|i| spawn(NetId::runtime_in(A, 1000 + i), "Part", "P", s.root)).collect();
                s.instances.extend(extra);
            }),
            Err(Refusal::Invalid)
        );

        // Too many bytes: refused when it is offered, and when it is read.
        let mut big = car(0.0);
        let filler = WireValue::String("x".repeat(60 * 1024));
        for i in 0..24u64 {
            let mut op = spawn(NetId::runtime_in(A, 2000 + i), "StringValue", "V", big.root);
            op.props.push(("Value".into(), filler.clone()));
            big.instances.push(op);
        }
        assert_eq!(big.check(), Ok(()), "each part is within bounds; only the total is not");
        let mut a = Handoffs::new(A, HandoffConfig::default());
        a.claim(big.root);
        assert_eq!(a.begin(big.root, neighbour(B), big.clone(), 0), Err(BeginError::TooLarge));
        assert_eq!(a.authority(big.root), Some(Authority::Owned { epoch: 0 }), "a refused begin changed authority");
        let frame = encode_frame(&HandoffMsg::Offer { id: HandoffId::new(A, 1), epoch: 0, snapshot: big }).unwrap();
        assert!(matches!(decode_message(&frame[4..]), Err(WireError::TooLarge(_))));
    }

    #[test]
    fn begin_needs_ownership_and_a_real_neighbour() {
        let mut a = Handoffs::new(A, HandoffConfig::default());
        let snap = car(0.0);
        assert_eq!(a.begin(snap.root, neighbour(B), snap.clone(), 0), Err(BeginError::NotHeld));
        a.claim(snap.root);
        assert_eq!(a.begin(snap.root, neighbour(A), snap.clone(), 0), Err(BeginError::BadNeighbour));
        assert_eq!(a.begin(snap.root, neighbour(0), snap.clone(), 0), Err(BeginError::BadNeighbour));
        assert!(a.begin(snap.root, neighbour(B), snap.clone(), 0).is_ok());
        assert_eq!(a.begin(snap.root, neighbour(3), snap, 1), Err(BeginError::Busy));
    }

    /// Every message's place in its enum is its tag on the shard channel.
    #[test]
    fn handoff_tags_never_move() {
        let tag = |m: &HandoffMsg| u32::from_le_bytes(encode_message(m).unwrap()[4..8].try_into().unwrap());
        let id = HandoffId::new(A, 1);
        let msgs = [
            HandoffMsg::Offer { id, epoch: 0, snapshot: car(0.0) },
            HandoffMsg::Accept { id, epoch: 0 },
            HandoffMsg::Refuse { id, epoch: 0, reason: Refusal::Full },
            HandoffMsg::Commit { id, epoch: 0, snapshot: car(0.0) },
            HandoffMsg::Committed { id, epoch: 0 },
            HandoffMsg::Abort { id, epoch: 0 },
        ];
        assert_eq!(msgs.iter().map(tag).collect::<Vec<_>>(), (0..6).collect::<Vec<u32>>());
        for m in &msgs {
            assert_eq!(&wire(m), m, "changed on the way through the codec");
        }
        let reasons = [
            Refusal::Full,
            Refusal::IdTaken,
            Refusal::Stale,
            Refusal::AlreadyHeld,
            Refusal::Busy,
            Refusal::Invalid,
            Refusal::NotHeld,
            Refusal::UnknownParent,
            Refusal::Closed,
        ];
        let reason_tag = |r: &Refusal| {
            let frame = encode_message(&HandoffMsg::Refuse { id, epoch: 0, reason: *r }).unwrap();
            // Variant tag, id, epoch, then the reason's own tag.
            u32::from_le_bytes(frame[4 + 4 + 8 + 8..4 + 4 + 8 + 8 + 4].try_into().unwrap())
        };
        assert_eq!(reasons.iter().map(reason_tag).collect::<Vec<_>>(), (0..9).collect::<Vec<u32>>());
    }

    #[test]
    fn the_neighbour_is_the_workers_first_choice_for_the_area() {
        let row = |id: &str, area: &str, full: bool, origin: u32| LiveRow {
            id: id.into(),
            full,
            link: Some(format!("eustress-player://join/{id}")),
            cert_sha256: Some("CD".repeat(32)),
            area: Some(area.into()),
            origin: Some(origin),
        };
        // In the Worker's order. Only the first eligible row is taken.
        let rows = vec![
            row("me", "lyon", false, 1),
            row("full", "lyon", true, 2),
            row("paris", "paris", false, 3),
            row("lyon-a", "lyon", false, 4),
            row("lyon-b", "lyon", false, 5),
        ];
        let n = pick_neighbour(&rows, "lyon", "me").expect("a neighbour");
        assert_eq!(n.server_id, "lyon-a");
        assert_eq!(n.origin, 4);
        assert_eq!(n.cert_sha256, "cd".repeat(32));
        assert_eq!(pick_neighbour(&rows, "marseille", "me"), None);
        // A row without an origin or a pin cannot take a handoff.
        let mut no_origin = rows.clone();
        no_origin[3].origin = None;
        no_origin[4].cert_sha256 = Some("short".into());
        assert_eq!(pick_neighbour(&no_origin, "lyon", "me"), None);
    }

    // ── Ids across hosts (audit M2) ─────────────────────────────────────────

    fn host(origin: Origin) -> (DataModel, HostReplicator) {
        let mut dm = DataModel::new();
        let root = dm.root();
        for class in ["Workspace", "ReplicatedStorage", "ServerStorage", "Players"] {
            dm.create_virtual(class, class, Some(root));
        }
        let mut host = HostReplicator::new(&dm);
        host.set_origin(origin).expect("a valid origin");
        host.bind_tree(&dm, &|_| None);
        (dm, host)
    }

    fn frame(host: &mut HostReplicator, dm: &mut DataModel) -> Outgoing {
        let dirty = dm.take_dirty();
        let _ = (dm.take_spawns(), dm.take_reparents(), dm.take_despawns());
        let out = host.observe(dm, &dirty, FrameEffects::default());
        let cursor = dm.event_cursor();
        dm.trim_events(cursor);
        dm.end_frame();
        out
    }

    fn spawned_ids(out: &Outgoing) -> Vec<NetId> {
        out.iter()
            .filter_map(|(_, op)| match op {
                ReplOp::Spawn(s) => Some(s.id),
                _ => None,
            })
            .collect()
    }

    /// A car made on A, a host's script having built it at run time.
    fn build_car(dm: &mut DataModel) -> InstanceId {
        let ws = dm.find_service("Workspace").unwrap();
        let model = dm.create("Model");
        dm.rename(model, "Car").unwrap();
        let body = dm.create("Part");
        dm.rename(body, "Body").unwrap();
        dm.set_prop(body, "CFrame", DmValue::CFrame(CFrame::from_axis_angle(Vector3 { x: 0.0, y: 1.0, z: 0.0 }, 0.3))).unwrap();
        dm.set_parent(body, Some(model)).unwrap();
        dm.set_prop(model, "PrimaryPart", DmValue::Instance(body)).unwrap();
        dm.set_attribute(model, "Fuel", DmValue::Number(0.5)).unwrap();
        dm.set_parent(model, Some(ws)).unwrap();
        model
    }

    #[test]
    fn an_instance_keeps_its_id_across_hosts() {
        let (mut dm_a, mut host_a) = host(A);
        let (mut dm_b, mut host_b) = host(B);
        let car_a = build_car(&mut dm_a);
        let out = frame(&mut host_a, &mut dm_a);
        let car_net = host_a.net_of(car_a).expect("the car has an id");
        assert_eq!(car_net.origin(), Some(A), "A's ids must carry A's origin");
        let snap = snapshot_of(&host_a, &dm_a, car_a, &[], 7).expect("players have the car");
        assert_eq!(snap.root, car_net);
        let ids: Vec<NetId> = snap.ids().collect();
        assert_eq!(ids, spawned_ids(&out), "the snapshot carries what players were sent");

        // B makes it under the same ids, and its players are sent those ids.
        assert_eq!(fits(&host_b, &dm_b, &snap), Ok(()));
        let made = materialize(&mut dm_b, &mut host_b, &snap).expect("made on B");
        let out_b = frame(&mut host_b, &mut dm_b);
        assert_eq!(spawned_ids(&out_b), ids, "B renamed what it was handed");
        let car_b = made[0].1;
        let body_b = made[1].1;
        assert_eq!(dm_b.get_prop(car_b, "PrimaryPart"), Some(DmValue::Instance(body_b)), "a reference inside the snapshot was lost");
        assert_eq!(dm_b.get_attribute(car_b, "Fuel"), Some(DmValue::Number(0.5)));
        // A player joining B late is sent it under the same ids too.
        let late = host_b.snapshot(&dm_b, None);
        for id in &ids {
            assert!(late.ops.iter().any(|op| matches!(op, ReplOp::Spawn(s) if s.id == *id)), "{id:?} missing for a late joiner");
        }
        // B's own new instances never take one of A's ids.
        let ws_b = dm_b.find_service("Workspace").unwrap();
        let own = dm_b.create("Part");
        dm_b.set_parent(own, Some(ws_b)).unwrap();
        frame(&mut host_b, &mut dm_b);
        let own_net = host_b.net_of(own).unwrap();
        assert_eq!(own_net.origin(), Some(B));
        assert!(!ids.contains(&own_net));

        // A releases its copy; the car comes back later under the same ids,
        // and A's own counter never mints over them.
        dm_a.destroy(car_a);
        frame(&mut host_a, &mut dm_a);
        let back = snapshot_of(&host_b, &dm_b, car_b, &[], 9).unwrap();
        assert_eq!(back.ids().collect::<Vec<_>>(), ids);
        materialize(&mut dm_a, &mut host_a, &back).expect("made on A again");
        assert_eq!(spawned_ids(&frame(&mut host_a, &mut dm_a)), ids);
        let ws_a = dm_a.find_service("Workspace").unwrap();
        let fresh = dm_a.create("Part");
        dm_a.set_parent(fresh, Some(ws_a)).unwrap();
        frame(&mut host_a, &mut dm_a);
        assert!(!ids.contains(&host_a.net_of(fresh).unwrap()));
    }

    /// Two hosts without origins mint the same ids. The destination refuses
    /// rather than bind one id to two instances.
    #[test]
    fn an_id_taken_on_the_destination_is_refused() {
        let (mut dm_a, mut host_a) = host(0);
        let (mut dm_b, mut host_b) = host(0);
        let ws_b = dm_b.find_service("Workspace").unwrap();
        let mine = dm_b.create("Part");
        dm_b.set_parent(mine, Some(ws_b)).unwrap();
        frame(&mut host_b, &mut dm_b);
        let car_a = build_car(&mut dm_a);
        frame(&mut host_a, &mut dm_a);
        let snap = snapshot_of(&host_a, &dm_a, car_a, &[], 1).unwrap();
        assert_eq!(fits(&host_b, &dm_b, &snap), Err(Refusal::IdTaken));
        assert_eq!(materialize(&mut dm_b, &mut host_b, &snap), Err(Refusal::IdTaken));
        assert_eq!(host_b.net_of(mine), Some(snap.root), "the refusal disturbed B's own instance");
        assert!(host_b.adopt(snap.root, InstanceId(999_999)).is_err());

        // A parent the destination does not have is refused too.
        let (dm_c, host_c) = host(3);
        let mut orphan = snap.clone();
        orphan.instances[0].parent = NetId::runtime_in(9, 1);
        assert_eq!(fits(&host_c, &dm_c, &orphan), Err(Refusal::UnknownParent));
        assert_eq!(host_a.set_origin(NetId::MAX_ORIGIN + 1), Err(NetId::MAX_ORIGIN + 1));
    }
}
