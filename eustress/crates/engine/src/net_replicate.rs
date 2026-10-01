//! # Server authority on the host
//!
//! While Studio hosts a session and Play runs, every player receives what
//! the host's scripts and physics do (`docs/networking/SERVER_AUTHORITY.md`):
//!
//! ```text
//! Play starts, hosting      every instance in the Play tree gets its NetId
//! each frame, after apply   the tree's changes -> SendWorld, per player
//! a player arrives          its catch-up first (HostReplicator::snapshot)
//! 30 times a second         bodies physics moved -> SendMotion; a body that
//!                           comes to rest gets its resting pose reliably
//! a player's track changes  checked, applied here, relayed to the others;
//!                           the host's own scripts' tracks go to everyone
//! ```
//!
//! A scene instance is identified by the record key players load it from:
//! the Space-relative path of its file, or, for a core the database streams,
//! the key the host's export published it at ([`HostWorldKeys`]).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use avian3d::prelude::{AngularVelocity, LinearVelocity, Position, RigidBody, Rotation, Sleeping};
use bevy::prelude::*;

use eustress_common::datamodel::{DataModel, DmValue, InstanceId, RemoteDelivery, RemoteTarget, RemoteValue};
use eustress_common::scripting::{CFrame, Vector3};
use eustress_networking::repl::motion::{split_motion, tick_at, TICK_HZ};
use eustress_networking::repl::remote::check_signature;
use eustress_networking::repl::{
    split_frame, Audience, BodyState, ClockLink, FrameEffects, HostReplicator, HostTracks, NetId, PreHostRecord,
    ReplOp, ReplicationTap, WireValue, WorldLayout, MAX_WORLD_FRAME_WEIGHT,
};
use eustress_networking::session::{RemoteArrived, SendMotion, SendRemoteReply, SendWorld, TracksArrived};
use eustress_networking::wire::{PeerId, HOST_PEER};
use eustress_networking::HostSession;

use crate::play_datamodel::remote_players::RemotePlayers;
use crate::play_datamodel::{PlayDataModel, PlayScriptSet};

/// Motion samples per second.
const MOTION_HZ: f32 = 30.0;
/// Bodies sent per sample at most; the ones waiting longest go first.
const MAX_BODIES_PER_SAMPLE: usize = 256;
/// Samples a moving body may stay unchanged before its resting pose is sent
/// reliably, so a lost datagram cannot leave it slightly off for good.
const SETTLE_SAMPLES: u32 = 15;

/// What the host's export knows about identity: the Space folder record keys
/// are relative to, and each streamed core's record key by stored id.
#[derive(Resource, Clone, Default)]
pub struct HostWorldKeys {
    pub space_root: PathBuf,
    pub cores: HashMap<u64, String>,
    /// What players load with the world, when hosting began in a Play
    /// session already running (see `HostReplicator::seed_before_hosting`).
    pub world: Option<Arc<WorldLayout>>,
}

/// What the running Play session changed before it was hosted, from Play to
/// Stop: names only (`PreHostRecord`), so a late joiner gets the values.
#[derive(Resource)]
struct BeforeHosting {
    session: Weak<parking_lot::Mutex<DataModel>>,
    record: PreHostRecord,
}

struct Moving {
    last: BodyState,
    sent_tick: u64,
    still: u32,
    settled: bool,
}

/// Replication for the running Play session.
#[derive(Resource)]
pub struct HostReplication {
    session: Weak<parking_lot::Mutex<DataModel>>,
    replicator: HostReplicator,
    caught_up: HashSet<PeerId>,
    motion_accum: f32,
    moving: HashMap<NetId, Moving>,
    tracks: HostTracks,
    /// Players' track changes applied this frame, for the others.
    track_relays: Vec<(Audience, ReplOp)>,
}

pub struct ReplicationPlugin;

impl Plugin for ReplicationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (fold_before_hosting, begin_replication, replicate_frame)
                .chain()
                .in_set(PlayScriptSet::Apply)
                .after(crate::play_datamodel::apply::apply_frame)
        )
        .add_systems(Update, open_play_tap.in_set(PlayScriptSet::Pull))
        .add_systems(Update, (pull_player_input, deliver_remote_calls).in_set(PlayScriptSet::Pull))
        // On this frame's clock, before the Animator steps.
        .add_systems(
            Update,
            deliver_player_tracks.in_set(PlayScriptSet::Pull).after(crate::play_datamodel::pull::pull_frame_state),
        )
        .add_systems(Update, end_replication);
    }
}

/// Where the Play tree's clock stands against the session's ticks now.
fn clock_link(g: &DataModel, secs: f64) -> ClockLink {
    ClockLink { frame_now: g.frame.time, tick_now: secs * TICK_HZ }
}

/// Joined players' animation track changes: each checked (an Animator in
/// that player's character, a clip in the published world, its rate),
/// applied to the tree so its Animator plays it here, and kept for the
/// other players.
fn deliver_player_tracks(
    time: Res<Time>,
    mut arrivals: MessageReader<TracksArrived>,
    rep: Option<ResMut<HostReplication>>,
    play: Option<Res<PlayDataModel>>,
    remote_players: Option<Res<RemotePlayers>>,
) {
    let (Some(mut rep), Some(play), Some(remote_players)) = (rep, play, remote_players) else {
        arrivals.clear();
        return;
    };
    let rep = &mut *rep;
    let now = time.elapsed_secs_f64();
    let mut g = play.dm.lock();
    let clock = clock_link(&g, now);
    for arrival in arrivals.read() {
        let player = remote_players.by_peer(arrival.peer).and_then(|p| p.instance());
        let applied = rep.tracks.player_ops(&mut g, &rep.replicator, arrival.peer, player, arrival.ops.clone(), clock, now);
        if applied.refused > 0 {
            debug!("multiplayer: refused {} animation change(s) from peer {}", applied.refused, arrival.peer);
        }
        rep.track_relays.extend(applied.out);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Remote calls
// ─────────────────────────────────────────────────────────────────────────────

/// A script's remote arguments on the wire. `net_of` names an instance the
/// receiver can see, or `None` (it arrives as nil).
fn to_wire(v: &RemoteValue, net_of: &dyn Fn(InstanceId) -> Option<NetId>) -> WireValue {
    match v {
        RemoteValue::Value(d) => WireValue::from_dm(d, net_of),
        RemoteValue::Array(items) => WireValue::Array(items.iter().map(|i| to_wire(i, net_of)).collect()),
        RemoteValue::Map(entries) => WireValue::Map(entries.iter().map(|(k, v)| (k.clone(), to_wire(v, net_of))).collect()),
    }
}

/// A player's arguments for this tree's scripts.
fn from_wire(v: &WireValue, local_of: &dyn Fn(NetId) -> Option<InstanceId>) -> RemoteValue {
    match v {
        WireValue::Array(items) => RemoteValue::Array(items.iter().map(|i| from_wire(i, local_of)).collect()),
        WireValue::Map(entries) => RemoteValue::Map(entries.iter().map(|(k, v)| (k.clone(), from_wire(v, local_of))).collect()),
        other => RemoteValue::Value(other.to_dm(local_of).unwrap_or(DmValue::Nil)),
    }
}

/// An `InvokeServer` answered on this host is identified by the player and
/// its own call number.
fn invocation_of(peer: PeerId, call: u32) -> u64 {
    ((peer as u64) << 32) | call as u64
}

/// Joined players' `FireServer` and `InvokeServer` calls, into the tree for
/// the scripts: only for a remote the player can see, naming only instances
/// it can see, and matching the remote's `ArgumentTypes` when it declares
/// them. (Size and rate were checked by the session.)
fn deliver_remote_calls(
    mut arrivals: MessageReader<RemoteArrived>,
    rep: Option<Res<HostReplication>>,
    play: Option<Res<PlayDataModel>>,
    remote_players: Option<Res<RemotePlayers>>,
) {
    let (Some(rep), Some(play), Some(remote_players)) = (rep, play, remote_players) else {
        arrivals.clear();
        return;
    };
    let mut g = play.dm.lock();
    for arrival in arrivals.read() {
        let Some(player) = remote_players.by_peer(arrival.peer).and_then(|p| p.instance()) else { continue };
        let call = &arrival.call;
        let r = &rep.replicator;
        let Some(remote) = r.local_of(call.remote).filter(|_| r.visible_to(call.remote, player)) else { continue };
        let is_remote = matches!(g.class_of(remote), Some("RemoteEvent" | "UnreliableRemoteEvent" | "RemoteFunction"));
        if !is_remote {
            continue;
        }
        let mut named = Vec::new();
        call.args.iter().for_each(|a| a.instances(&mut named));
        if named.iter().any(|n| !r.visible_to(*n, player)) {
            continue;
        }
        if let Err(e) = check_declared(&g, remote, &call.args) {
            debug!("multiplayer: refused a call to {} from peer {}: {e}", g.full_name(remote), arrival.peer);
            continue;
        }
        let args = call.args.iter().map(|a| from_wire(a, &|n| r.local_of(n))).collect();
        let invocation = (call.call != 0).then(|| invocation_of(arrival.peer, call.call));
        g.remote_in.push(RemoteDelivery { remote, from: Some(player), args, invocation });
    }
}

/// A player's call to `remote`, refused when it does not match the types the
/// host's scripts declared for it (`RemoteEvent:SetArgumentTypes`, kept in
/// the remote's `ArgumentTypes` property as comma separated names). A remote
/// that declared nothing takes any arguments within the size limits; one
/// that declared an empty list (`SetArgumentTypes({})`, an empty
/// `ArgumentTypes`) takes none.
fn check_declared(g: &DataModel, remote: InstanceId, args: &[WireValue]) -> Result<(), String> {
    let Some(declared) = g.get_prop(remote, "ArgumentTypes").and_then(|v| v.as_str().map(str::to_string)) else {
        return Ok(());
    };
    let signature: Vec<String> =
        declared.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
    check_signature(args, &signature)
}

/// This frame's `FireClient` / `FireAllClients` calls, as ops ordered after
/// the frame's writes; and answers to invocations, for the session to send.
fn drain_remote_calls(g: &mut DataModel, rep: &HostReplication, out: &mut Vec<(Audience, ReplOp)>) -> Vec<SendRemoteReply> {
    let r = &rep.replicator;
    for call in std::mem::take(&mut g.remote_out) {
        let audience = match call.target {
            RemoteTarget::AllClients => Audience::Everyone,
            RemoteTarget::Client(player) => Audience::Owner(player),
            // The host is the server: a call to it never leaves.
            RemoteTarget::Server => continue,
        };
        let sees = |n: NetId| r.seen_by(n, audience);
        let Some(remote) = r.net_of(call.remote).filter(|n| sees(*n)) else {
            debug!("multiplayer: {} is not replicated, so its call stays on the host", g.full_name(call.remote));
            continue;
        };
        let args = call.args.iter().map(|a| to_wire(a, &|id| r.net_of(id).filter(|n| sees(*n)))).collect();
        out.push((audience, ReplOp::Remote { remote, args }));
    }
    let mut replies = Vec::new();
    for reply in std::mem::take(&mut g.reply_out) {
        let peer = (reply.invocation >> 32) as PeerId;
        let call = reply.invocation as u32;
        let player = reply.to;
        let result = reply.result.map(|values| {
            values
                .iter()
                .map(|v| to_wire(v, &|id| r.net_of(id).filter(|n| player.is_some_and(|p| r.visible_to(*n, p)))))
                .collect()
        });
        replies.push(SendRemoteReply { peer, reply: eustress_networking::repl::RemoteReply { call, result } });
    }
    replies
}

/// Each joined player's newest input, into the tree, where `Player:IsKeyDown`
/// reads it. A player with none yet holds nothing.
fn pull_player_input(
    host: Option<Res<HostSession>>,
    play: Option<Res<PlayDataModel>>,
    remote: Option<Res<RemotePlayers>>,
    inputs: Res<eustress_networking::repl::PeerInputs>,
) {
    let (Some(_), Some(play)) = (host, play) else { return };
    let mut g = play.dm.lock();
    let mut now: HashMap<InstanceId, eustress_common::datamodel::InputState> = HashMap::new();
    for p in remote.iter().flat_map(|r| r.players()) {
        let Some(instance) = p.instance() else { continue };
        let mut state = eustress_common::datamodel::InputState::default();
        if let Some(sample) = inputs.latest(p.peer) {
            use eustress_networking::repl::input::{KEYS, MOUSE_LEFT, MOUSE_MIDDLE, MOUSE_RIGHT};
            state.keys = KEYS.iter().enumerate().filter(|(bit, _)| sample.key(*bit)).map(|(_, (_, name))| name.to_string()).collect();
            for (bit, name) in [(MOUSE_LEFT, "MouseButton1"), (MOUSE_RIGHT, "MouseButton2"), (MOUSE_MIDDLE, "MouseButton3")] {
                if sample.mouse & bit != 0 {
                    state.buttons.insert(name.to_string());
                }
            }
        }
        now.insert(instance, state);
    }
    g.player_input = now;
}

/// The Space-relative key of an instance loaded from `path`.
fn relative_key(path: &Path, space_root: &Path) -> String {
    path.strip_prefix(space_root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// The record key the entity bound to `id` was loaded from.
fn record_key(world: &World, dm: &DataModel, id: InstanceId, keys: &HostWorldKeys) -> Option<String> {
    let entity = Entity::from_bits(dm.entity_of(id)?);
    let e = world.get_entity(entity).ok()?;
    #[cfg(feature = "world-db")]
    if let Some(core) = e.get::<crate::space::world_db_binary::BinaryEcsInstance>() {
        if let Some(key) = keys.cores.get(&core.stored_id) {
            return Some(key.clone());
        }
    }
    if let Some(f) = e.get::<crate::space::instance_loader::InstanceFile>() {
        return Some(relative_key(&f.toml_path, &keys.space_root));
    }
    e.get::<crate::space::file_loader::LoadedFromFile>().map(|f| relative_key(&f.path, &keys.space_root))
}

/// Play has begun: the apply step keeps its writes in the tap from the first
/// frame, and the session keeps a record of what it changes before anyone
/// hosts it, so a late joiner gets that too.
fn open_play_tap(world: &mut World) {
    let Some(dm) = world.get_resource::<PlayDataModel>().map(|p| p.dm.clone()) else { return };
    if !world.contains_resource::<ReplicationTap>() {
        world.insert_resource(ReplicationTap::default());
    }
    let current = world
        .get_resource::<BeforeHosting>()
        .is_some_and(|b| b.session.upgrade().is_some_and(|s| Arc::ptr_eq(&s, &dm)));
    if !current {
        world.insert_resource(BeforeHosting { session: Arc::downgrade(&dm), record: PreHostRecord::default() });
    }
}

/// While nobody hosts the session: the names of what its scripts wrote this
/// frame. The tap is emptied every frame, so it never grows.
fn fold_before_hosting(
    play: Option<Res<PlayDataModel>>,
    tap: Option<ResMut<ReplicationTap>>,
    before: Option<ResMut<BeforeHosting>>,
    rep: Option<Res<HostReplication>>,
) {
    let (Some(play), Some(mut before)) = (play, before) else { return };
    if rep.is_none() {
        if let Some(mut tap) = tap {
            let dirty = std::mem::take(&mut tap.dirty);
            before.record.fold_writes(&dirty);
        }
    }
    before.record.fold_events(&play.dm.lock());
}

/// Hosting and playing, with no replication for this Play session yet:
/// bind every instance and start.
fn begin_replication(world: &mut World) {
    let (Some(_), Some(play)) = (world.get_resource::<HostSession>(), world.get_resource::<PlayDataModel>()) else {
        return;
    };
    let dm = play.dm.clone();
    let current = world
        .get_resource::<HostReplication>()
        .is_some_and(|r| r.session.upgrade().is_some_and(|s| Arc::ptr_eq(&s, &dm)));
    if current {
        return;
    }
    let keys = world.get_resource::<HostWorldKeys>().cloned().unwrap_or_default();
    let started = std::time::Instant::now();
    let mut replicator = {
        let mut g = dm.lock();
        let mut key_of: HashMap<InstanceId, String> = HashMap::new();
        for id in g.descendants(g.root()) {
            if let Some(key) = record_key(world, &g, id, &keys) {
                key_of.insert(id, key);
            }
        }
        let mut r = HostReplicator::new(&g);
        r.bind_tree(&g, &|id| key_of.get(&id).cloned());
        // What the session did before this moment, for late joiners.
        if let Some(before) = world.get_resource::<BeforeHosting>().filter(|b| b.session.upgrade().is_some_and(|s| Arc::ptr_eq(&s, &dm))) {
            r.seed_before_hosting(&g, keys.world.as_deref(), &|id| key_of.get(&id).cloned(), &before.record);
        }
        if let Some(local) = g.local_player {
            r.set_player(local, HOST_PEER);
        }
        // FireClient and FireAllClients now reach joined players.
        g.networked = true;
        r
    };
    for problem in replicator.problems.drain(..) {
        warn!("multiplayer: {problem}");
    }
    if let Some(remote) = world.get_resource::<RemotePlayers>() {
        for p in remote.players() {
            if let Some(instance) = p.instance() {
                replicator.set_player(instance, p.peer);
            }
        }
    }
    info!("multiplayer: replicating the Play session ({} ms to bind)", started.elapsed().as_millis());
    if !world.contains_resource::<ReplicationTap>() {
        world.insert_resource(ReplicationTap::default());
    }
    world.insert_resource(HostReplication {
        session: Arc::downgrade(&dm),
        replicator,
        caught_up: HashSet::new(),
        motion_accum: 0.0,
        moving: HashMap::new(),
        tracks: HostTracks::default(),
        track_relays: Vec::new(),
    });
}

/// Send this frame's changes: catch-ups to players who just arrived, live
/// frames to the rest, and motion when a sample is due.
fn replicate_frame(world: &mut World) {
    let Some(mut rep) = world.remove_resource::<HostReplication>() else { return };
    let Some(dm) = rep.session.upgrade() else { return };
    let dirty = world.get_resource_mut::<ReplicationTap>().map(|mut t| std::mem::take(&mut t.dirty)).unwrap_or_default();
    // Kept for a later host in this session, should this one stop.
    if let Some(mut before) = world.get_resource_mut::<BeforeHosting>() {
        before.record.fold_writes(&dirty);
    }
    let (secs, dt) = {
        let time = world.resource::<Time>();
        (time.elapsed_secs_f64(), time.delta_secs())
    };
    let tick = tick_at(secs);
    rep.replicator.set_tick(tick);

    // Players who joined since the last frame.
    let players: Vec<(PeerId, Option<InstanceId>)> = {
        let remote = world.get_resource::<RemotePlayers>();
        let ready = world.get_resource::<HostSession>().map(|h| h.ready_peers()).unwrap_or_default();
        ready
            .into_iter()
            .map(|(peer, _)| (peer, remote.and_then(|r| r.by_peer(peer)).and_then(|p| p.instance())))
            .collect()
    };
    // Every joined player's `Player`, ready or still downloading, so the one
    // made for a player mid-download reaches the others naming its peer.
    if let Some(remote) = world.get_resource::<RemotePlayers>() {
        for p in remote.players() {
            if let Some(i) = p.instance() {
                rep.replicator.set_player(i, p.peer);
            }
        }
    }
    rep.caught_up.retain(|p| players.iter().any(|(q, _)| q == p));

    let mut out = {
        let mut g = dm.lock();
        let effects = FrameEffects { emits: &g.particle_emits };
        let mut out = rep.replicator.observe(&g, &dirty, effects);
        // Animation tracks after the structure, so an Animator made this
        // frame is on players before a track names it.
        let clock = clock_link(&g, secs);
        out.extend(rep.tracks.host_ops(&mut g, &rep.replicator, clock));
        out.extend(std::mem::take(&mut rep.track_relays));
        out
    };

    rep.motion_accum += dt;
    let mut motion = Vec::new();
    if rep.motion_accum >= 1.0 / MOTION_HZ {
        rep.motion_accum = (rep.motion_accum - 1.0 / MOTION_HZ).min(1.0 / MOTION_HZ);
        motion = sample_motion(world, &dm, &mut rep, tick, &mut out);
    }
    // Last, so a RemoteEvent reaches players after the writes made before it.
    let replies = drain_remote_calls(&mut dm.lock(), &rep, &mut out);

    let mut sends: Vec<SendWorld> = Vec::new();
    {
        let g = dm.lock();
        for (peer, instance) in &players {
            if rep.caught_up.insert(*peer) {
                // Its catch-up is built after this frame's changes, so it
                // includes them: this frame's live ops are not for it.
                let mut catch_up = rep.replicator.snapshot(&g, *instance);
                catch_up.ops.extend(rep.tracks.snapshot(&g, &rep.replicator, *instance, clock_link(&g, secs)));
                let frames = split_frame(catch_up, MAX_WORLD_FRAME_WEIGHT);
                info!("multiplayer: catching peer {peer} up ({} ops)", frames.iter().map(|f| f.ops.len()).sum::<usize>());
                sends.push(SendWorld { peer: *peer, frames, catch_up: true });
            } else if !out.is_empty() {
                let frames = rep.replicator.frames_for(&out, *instance, MAX_WORLD_FRAME_WEIGHT);
                if !frames.is_empty() {
                    sends.push(SendWorld { peer: *peer, frames, catch_up: false });
                }
            }
        }
    }
    for s in sends {
        world.write_message(s);
    }
    for r in replies {
        world.write_message(r);
    }
    if !motion.is_empty() {
        world.write_message(SendMotion { frames: split_motion(tick, &motion) });
    }
    world.insert_resource(rep);
}

/// The bodies physics moved since the last sample that every player has,
/// as motion; and, into `out`, the resting pose of each that stopped.
fn sample_motion(
    world: &mut World,
    dm: &Arc<parking_lot::Mutex<DataModel>>,
    rep: &mut HostReplication,
    tick: u64,
    out: &mut Vec<(Audience, ReplOp)>,
) -> Vec<BodyState> {
    let g = dm.lock();
    let mut seen: HashSet<NetId> = HashSet::new();
    let mut changed: Vec<(u64, BodyState)> = Vec::new();
    let mut q = world
        .query_filtered::<(Entity, &RigidBody, &Position, &Rotation, &LinearVelocity, &AngularVelocity), Without<Sleeping>>();
    for (entity, body, pos, rot, lin, ang) in q.iter(world) {
        if *body != RigidBody::Dynamic {
            continue;
        }
        let Some(net) = g.by_entity(entity.to_bits()).and_then(|id| rep.replicator.motion_id(id)) else { continue };
        seen.insert(net);
        let state = BodyState::new(net, pos.0, rot.0, lin.0, ang.0);
        match rep.moving.get_mut(&net) {
            Some(m) if !state.differs_from(&m.last) => {
                m.still += 1;
                if m.still == SETTLE_SAMPLES && !m.settled {
                    m.settled = true;
                    out.push((Audience::Everyone, settle_op(net, &state)));
                }
            }
            Some(m) => changed.push((m.sent_tick, state)),
            None => changed.push((0, state)),
        }
    }
    // Asleep or gone since the last sample: its resting pose, once.
    let stopped: Vec<NetId> = rep.moving.keys().filter(|n| !seen.contains(*n)).copied().collect();
    for net in stopped {
        if let Some(m) = rep.moving.remove(&net) {
            if !m.settled && rep.replicator.local_of(net).is_some_and(|id| g.exists(id)) {
                out.push((Audience::Everyone, settle_op(net, &m.last)));
            }
        }
    }
    // Longest waiting first, so a crowd of moving bodies shares the budget.
    changed.sort_by_key(|(sent, _)| *sent);
    changed.truncate(MAX_BODIES_PER_SAMPLE);
    let mut bodies = Vec::with_capacity(changed.len());
    for (_, state) in changed {
        rep.replicator.note_motion(state.id);
        rep.moving.insert(state.id, Moving { last: state, sent_tick: tick, still: 0, settled: false });
        bodies.push(state);
    }
    bodies
}

/// A body's pose as a reliable property write.
fn settle_op(net: NetId, state: &BodyState) -> ReplOp {
    let q = state.rotation();
    let p = state.position();
    let mut cf = CFrame::from_quaternion([q.x as f64, q.y as f64, q.z as f64, q.w as f64]);
    cf.position = Vector3::new(p.x as f64, p.y as f64, p.z as f64);
    let value = WireValue::from_dm(&DmValue::CFrame(cf), &|_| None);
    ReplOp::SetProps { id: net, props: vec![("CFrame".to_string(), value)] }
}

/// Hosting stopped, or Play did: nothing left to replicate. The tap and the
/// record of what the session changed last as long as Play does.
fn end_replication(
    mut commands: Commands,
    host: Option<Res<HostSession>>,
    play: Option<Res<PlayDataModel>>,
    rep: Option<Res<HostReplication>>,
    kept: (Option<Res<ReplicationTap>>, Option<Res<BeforeHosting>>),
) {
    if rep.is_some() && (host.is_none() || play.is_none()) {
        commands.remove_resource::<HostReplication>();
        // Play goes on without the server: remote calls loop back again.
        if let Some(play) = &play {
            play.dm.lock().networked = false;
        }
    }
    if play.is_none() && (kept.0.is_some() || kept.1.is_some()) {
        commands.remove_resource::<ReplicationTap>();
        commands.remove_resource::<BeforeHosting>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::datamodel::OutputLevel;
    use eustress_common::luau::play::{PlayLuau, RayHit, RayQuery, ScriptLaunch, TerrainReadFn};

    fn no_rays(_: &RayQuery) -> Option<RayHit> {
        None
    }

    /// The whole path, from a server script to the refusal: the script
    /// declares a remote's argument types in Luau, and a player's call that
    /// does not match them is refused before any script would see it.
    #[test]
    fn types_a_server_script_declares_refuse_a_mismatched_call() {
        let dm = eustress_common::datamodel::new_shared();
        let (script, place, aim) = {
            let mut g = dm.lock();
            let storage = g.get_service("ReplicatedStorage").expect("ReplicatedStorage");
            let place = g.create_virtual("RemoteEvent", "PlaceBlock", Some(storage));
            let aim = g.create_virtual("UnreliableRemoteEvent", "Aim", Some(storage));
            let service = g.get_service("ServerScriptService").expect("ServerScriptService");
            (g.create_virtual("Script", "Builder", Some(service)), place, aim)
        };
        let mut vm = PlayLuau::new(dm.clone()).expect("the prelude loads");
        let source = r#"
            local storage = game:GetService("ReplicatedStorage")
            storage.PlaceBlock:SetArgumentTypes({"Vector3", "number?"})
            storage.Aim:SetArgumentTypes({"Vector3"})
        "#;
        let launch = ScriptLaunch { instance: script, source: source.into(), chunk_name: "Builder".into() };
        let no_terrain: &TerrainReadFn<'_> = &|_| {};
        vm.run_scripts(vec![launch], &no_rays, no_terrain);

        let g = dm.lock();
        assert!(g.output.iter().all(|l| l.level != OutputLevel::Error), "{:?}", g.output);
        let at = WireValue::Vector3([1.0, 2.0, 3.0]);
        assert!(check_declared(&g, place, &[at.clone(), WireValue::Number(2.0)]).is_ok());
        assert!(check_declared(&g, place, &[at.clone()]).is_ok(), "the second argument is optional");
        assert!(check_declared(&g, place, &[WireValue::String("x".into())]).is_err(), "a string where a Vector3 was declared");
        assert!(check_declared(&g, place, &[]).is_err(), "the required Vector3 missing");
        // An unreliable remote is held to what it declared the same way.
        assert!(check_declared(&g, aim, &[at]).is_ok());
        assert!(check_declared(&g, aim, &[WireValue::Number(1.0)]).is_err());
    }

    /// `SetArgumentTypes({})` declares a remote that takes no arguments,
    /// which is not the same as declaring nothing.
    #[test]
    fn an_empty_declaration_takes_no_arguments() {
        let mut g = DataModel::new();
        let storage = g.get_service("ReplicatedStorage").expect("ReplicatedStorage");
        let open = g.create_virtual("RemoteEvent", "Open", Some(storage));
        let enter_exit = g.create_virtual("RemoteEvent", "EnterExit", Some(storage));
        let spawn_car = g.create_virtual("RemoteEvent", "SpawnCar", Some(storage));
        g.set_prop(enter_exit, "ArgumentTypes", DmValue::String(String::new())).unwrap();
        g.set_prop(spawn_car, "ArgumentTypes", DmValue::String("string".into())).unwrap();
        let one = [WireValue::Number(1.0)];
        assert!(check_declared(&g, open, &one).is_ok(), "a remote that declared nothing takes anything");
        assert!(check_declared(&g, enter_exit, &[]).is_ok());
        assert!(check_declared(&g, enter_exit, &one).is_err(), "an empty declaration takes nothing");
        assert!(check_declared(&g, spawn_car, &[WireValue::String("28".into())]).is_ok());
        assert!(check_declared(&g, spawn_car, &one).is_err());
    }
}
