//! # The host's world, kept current on this Player
//!
//! Server authority on the Player's side (`docs/networking/SERVER_AUTHORITY.md`):
//!
//! ```text
//! WorldArrived    Replica::apply into PlayDataModel, before the draw step
//! MotionArrived   the host's moving bodies, drawn a little in the past,
//!                 between samples
//! remote_out      this Player's FireServer / InvokeServer, to the host
//! RemoteReplied   the host's answers, into reply_in
//! characters      each one's root is bound to the avatar drawn for it, and
//!                 its root and head follow that avatar; a character the
//!                 host seated rides the seat as this Player draws it
//! tracks          other machines' animation changes in (with the world
//!                 frames); this Player's own character's out (SendTracks)
//! ```
//!
//! The tree is bound once it exists together with [`SceneKeys`], the record
//! key of every scene instance the world's reader made; frames that arrive
//! earlier wait for it.
//!
//! Order within a frame matters. Everything that writes the tree runs before
//! the Play frame's `Pull` ([`PlayScriptSet`]), so the Player's LocalScripts
//! hear each host change as `Changed` or `ChildAdded` in the frame it
//! arrived. It runs after the avatar moves (`AvatarSystems::Locomotion`), so
//! a character's root follows this frame's pose, and so after the session
//! reads the network, which happens before the avatar's frame begins. Calls
//! the scripts make go out after `Scripts` and before the draw step
//! (`Apply`), in the same frame.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use bevy::prelude::*;

use eustress_common::animation::AnimatorSet;
use eustress_common::avatar::seat::{seat_offset, AvatarSeated};
use eustress_common::avatar::spawn::{AvatarBody, AvatarIntent};
use eustress_common::avatar::{AvatarSystems, LocalAvatar};
use eustress_common::datamodel::{
    DataModel, DmEvent, DmValue, InstanceId, RemoteDelivery, RemoteReply, RemoteTarget, RemoteValue, SharedDataModel,
};
use eustress_common::play_session::{FromTree, PlayDataModel, PlayScriptSet};
use eustress_common::scripting::{CFrame, Vector3};
use eustress_networking::repl::{ClockLink, Interpolator, NetId, ReplOp, Replica, WireValue, WorldFrame};
use eustress_networking::session::{
    FireRemote, MotionArrived, NetReplica, PlayerSession, RemoteReplied, SendTracks, WorldArrived,
};

/// Host ticks between motion samples (the host sends 30 a second).
const MOTION_INTERVAL_TICKS: f64 = 2.0;

/// The lock a [`SharedDataModel`] wraps.
type TreeLock = <SharedDataModel as std::ops::Deref>::Target;

/// The record key of every scene instance in [`PlayDataModel`], from the reader
/// that built it: the Space-relative path of the file each came from. The
/// shell inserts it together with the tree.
#[derive(Resource, Default, Clone)]
pub struct SceneKeys(pub Vec<(String, InstanceId)>);

/// This Player's replication state.
#[derive(Resource, Default)]
pub struct PlayerReplica {
    replica: Option<Replica>,
    /// The tree the replica is bound to.
    tree: Weak<TreeLock>,
    /// World frames that arrived before the tree was bound.
    waiting: Vec<WorldFrame>,
    motion: Interpolator,
}

pub struct NetReplicaPlugin;

impl Plugin for NetReplicaPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerReplica>()
            .add_systems(
                Update,
                (bind_tree, apply_world_frames, apply_motion, receive_replies, follow_characters)
                    .chain()
                    .after(AvatarSystems::Locomotion)
                    .before(PlayScriptSet::Pull)
                    .before(AnimatorSet::Step),
            )
            .add_systems(
                Update,
                (send_remote_calls, send_track_ops).after(PlayScriptSet::Scripts).before(PlayScriptSet::Apply),
            );
    }
}

/// Where this tree's `frame.time` stands against the host's tick now, once
/// the host's clock is known.
fn clock_link(g: &DataModel, session: Option<&PlayerSession>, now: f64) -> Option<ClockLink> {
    let tick_now = session?.host_tick(now)?;
    Some(ClockLink { frame_now: g.frame.time, tick_now })
}

/// Bind the tree on the first frame it and its keys exist and the host has
/// said which peer this Player is.
fn bind_tree(
    time: Res<Time>,
    tree: Option<Res<PlayDataModel>>,
    keys: Option<Res<SceneKeys>>,
    session: Option<Res<PlayerSession>>,
    mut state: ResMut<PlayerReplica>,
) {
    let (Some(tree), Some(keys), Some(peer)) = (tree, keys, session.as_ref().and_then(|s| s.peer())) else { return };
    if state.tree.upgrade().is_some_and(|t| Arc::ptr_eq(&t, &tree.dm)) {
        return;
    }
    let mut g = tree.dm.lock();
    let mut replica = Replica::new(&g);
    for (key, id) in &keys.0 {
        replica.bind_scene(key, *id);
    }
    if let Some(me) = g.local_player {
        replica.set_local_player(peer, me);
        // Scripts read `Players.LocalPlayer`: it names this tree's own
        // Player, which the host's Player for this peer lands on.
        if let Some(players) = g.find_service("Players") {
            if g.get_prop(players, "LocalPlayer").and_then(|v| v.as_instance()) != Some(me) {
                g.set_prop_from_engine(players, "LocalPlayer", DmValue::Instance(me));
            }
        }
    }
    // This tree is a client of the host: RunService:IsServer() is false, and
    // FireServer goes to the host instead of looping back.
    g.is_server = false;
    g.networked = true;
    // A new tree is a new world: the last one's bodies are gone.
    state.motion = Interpolator::default();
    replica.set_clock(clock_link(&g, session.as_deref(), time.elapsed_secs_f64()));
    let waiting = std::mem::take(&mut state.waiting);
    for frame in &waiting {
        deliver(&mut replica, &mut g, frame, &mut state.motion);
    }
    info!("net: replicating into the world's tree ({} scene instances, {} early frames)", keys.0.len(), waiting.len());
    state.replica = Some(replica);
    state.tree = Arc::downgrade(&tree.dm);
}

/// Apply one world frame, and hand its RemoteEvent calls to the scripts.
fn deliver(replica: &mut Replica, g: &mut DataModel, frame: &WorldFrame, motion: &mut Interpolator) {
    let applied = replica.apply(g, frame);
    for p in &applied.problems {
        debug!("net: a host change was not applied: {p}");
    }
    for (remote, args) in applied.remotes {
        let args = args.iter().map(|a| from_wire(a, &|n| replica.local_of(n))).collect();
        g.remote_in.push(RemoteDelivery { remote, from: None, args, invocation: None });
    }
    // A body the host placed reliably (it came to rest, or a script set it)
    // stops following the motion lane.
    for op in &frame.ops {
        match op {
            ReplOp::SetProps { id, props } if props.iter().any(|(n, _)| n == "CFrame") => motion.forget(*id),
            ReplOp::Destroy { id } => motion.forget(*id),
            _ => {}
        }
    }
}

fn apply_world_frames(
    time: Res<Time>,
    tree: Option<Res<PlayDataModel>>,
    session: Option<Res<PlayerSession>>,
    mut arrived: MessageReader<WorldArrived>,
    mut state: ResMut<PlayerReplica>,
) {
    let state = &mut *state;
    let bound = tree.as_ref().is_some_and(|t| state.tree.upgrade().is_some_and(|b| Arc::ptr_eq(&b, &t.dm)));
    let now = time.elapsed_secs_f64();
    for WorldArrived(frame) in arrived.read() {
        match (&mut state.replica, &tree) {
            (Some(replica), Some(tree)) if bound => {
                let mut g = tree.dm.lock();
                // Track changes in the frame convert their times with it.
                replica.set_clock(clock_link(&g, session.as_deref(), now));
                deliver(replica, &mut g, frame, &mut state.motion);
            }
            _ => state.waiting.push(frame.clone()),
        }
    }
}

/// This Player's scripts' animation track changes on its own character, to
/// the host, which plays them there and relays them to everyone else.
fn send_track_ops(
    time: Res<Time>,
    tree: Option<Res<PlayDataModel>>,
    session: Option<Res<PlayerSession>>,
    mut state: ResMut<PlayerReplica>,
    mut send: MessageWriter<SendTracks>,
) {
    let state = &mut *state;
    let Some(tree) = tree.filter(|t| state.tree.upgrade().is_some_and(|b| Arc::ptr_eq(&b, &t.dm))) else { return };
    let Some(replica) = state.replica.as_mut() else { return };
    let mut g = tree.dm.lock();
    // Until the host's clock is known the changes wait in the tree's log.
    let Some(clock) = clock_link(&g, session.as_deref(), time.elapsed_secs_f64()) else { return };
    let ops = replica.outgoing_tracks(&mut g, clock);
    if !ops.is_empty() {
        send.write(SendTracks(ops));
    }
}

/// Draw the host's moving bodies between samples, a little in the past, and
/// keep the tree's `CFrame` current for this Player's scripts.
fn apply_motion(
    time: Res<Time>,
    tree: Option<Res<PlayDataModel>>,
    session: Option<Res<PlayerSession>>,
    mut arrived: MessageReader<MotionArrived>,
    mut state: ResMut<PlayerReplica>,
    mut drawn: Query<&mut Transform, With<FromTree>>,
) {
    let state = &mut *state;
    for MotionArrived(frame) in arrived.read() {
        state.motion.push(frame);
    }
    let (Some(tree), Some(session), Some(replica)) = (tree, session, state.replica.as_ref()) else { return };
    let now = time.elapsed_secs_f64();
    let Some(host_tick) = session.host_tick(now) else { return };
    let at = host_tick - session.clock().delay_ticks(MOTION_INTERVAL_TICKS);
    let mut g = tree.dm.lock();
    let ids: Vec<NetId> = state.motion.ids().collect();
    for net in ids {
        let (Some((pos, rot)), Some(local)) = (state.motion.sample(net, at), replica.local_of(net)) else { continue };
        if let Some(bits) = g.entity_of(local) {
            if let Ok(mut tf) = drawn.get_mut(Entity::from_bits(bits)) {
                tf.translation = pos;
                tf.rotation = rot;
            }
        }
        let mut cf = CFrame::from_quaternion([rot.x as f64, rot.y as f64, rot.z as f64, rot.w as f64]);
        cf.position = Vector3::new(pos.x as f64, pos.y as f64, pos.z as f64);
        g.set_prop_from_engine(local, "CFrame", DmValue::CFrame(cf));
    }
}

/// Keep every character's `HumanoidRootPart` and `Head` where its avatar is
/// on this machine, for this Player's scripts: this Player's own avatar for
/// its own character, the avatar lane's replica for everyone else's. The
/// avatar lane already carries each pose, so the host never sends them.
fn follow_characters(
    mut commands: Commands,
    tree: Option<Res<PlayDataModel>>,
    state: Res<PlayerReplica>,
    own: Query<(Entity, &Transform, &AvatarBody, &AvatarIntent), With<LocalAvatar>>,
    others: Query<(Entity, &NetReplica, &Transform, &AvatarBody, &AvatarIntent)>,
    drawn: Query<(), With<FromTree>>,
    riding: Query<&AvatarSeated>,
    mut seat_of: Local<HashMap<InstanceId, InstanceId>>,
) {
    let (Some(tree), Some(replica)) = (tree, state.replica.as_ref()) else { return };
    let mut g = tree.dm.lock();
    let Some(players) = g.find_service("Players") else { return };
    for player in g.children(players).to_vec() {
        let Some(model) = g.get_prop(player, "Character").and_then(|v| v.as_instance()).filter(|m| g.exists(*m)) else {
            continue;
        };
        // The host seats characters; this Player hears it as `Seated`, as the
        // host's scripts did.
        let humanoid = g.find_first_child_of_class(model, "Humanoid", false);
        let seat = humanoid
            .and_then(|h| g.get_prop(h, "SeatPart"))
            .and_then(|v| v.as_instance())
            .filter(|s| g.exists(*s));
        if let Some(h) = humanoid {
            if seat_of.get(&h).copied() != seat {
                let args = match seat {
                    Some(s) => {
                        seat_of.insert(h, s);
                        vec![DmValue::Bool(true), DmValue::Instance(s)]
                    }
                    None => {
                        seat_of.remove(&h);
                        vec![DmValue::Bool(false), DmValue::Nil]
                    }
                };
                g.push_event(DmEvent::Signal { id: h, name: "Seated".into(), args });
            }
        }
        let avatar = if Some(player) == g.local_player {
            own.iter().next()
        } else {
            replica
                .peer_of(player)
                .and_then(|peer| others.iter().find(|(_, r, ..)| r.peer == peer))
                .map(|(e, _, tf, body, intent)| (e, tf, body, intent))
        };
        let Some((avatar, tf, body, intent)) = avatar else { continue };
        let mut root_cf = CFrame::from_quaternion([tf.rotation.x as f64, tf.rotation.y as f64, tf.rotation.z as f64, tf.rotation.w as f64]);
        root_cf.position = Vector3::new(tf.translation.x as f64, tf.translation.y as f64, tf.translation.z as f64);
        if let Some(root) = g.find_first_child(model, "HumanoidRootPart", false) {
            bind_root(&mut commands, &mut g, root, avatar, &drawn);
            g.set_prop_from_engine(root, "CFrame", DmValue::CFrame(root_cf));
        }
        if let Some(head) = g.find_first_child(model, "Head", false) {
            let mut head_cf = root_cf;
            let eye = (body.metrics.eye_height - body.metrics.capsule_half_extent()) as f64;
            head_cf.position = root_cf.position + Vector3::new(0.0, eye, 0.0);
            g.set_prop_from_engine(head, "CFrame", DmValue::CFrame(head_cf));
        }
        if let Some(humanoid) = humanoid {
            let d = intent.direction;
            g.set_prop_from_engine(humanoid, "MoveDirection", DmValue::Vector3(Vector3::new(d.x as f64, 0.0, d.z as f64)));
        }
        // The avatar rides the seat where this Player draws it.
        let seat_entity = seat.and_then(|s| g.entity_of(s)).map(Entity::from_bits);
        let riding_now = riding.get(avatar).ok().map(|r| r.seat);
        match (seat, seat_entity) {
            (Some(s), Some(entity)) if riding_now != Some(entity) => {
                let size_y = g.get_prop(s, "Size").and_then(|v| v.as_vector3()).map_or(1.0, |v| v.y as f32);
                commands.entity(avatar).insert(AvatarSeated { seat: entity, offset: seat_offset(size_y, &body.metrics) });
            }
            (None, _) if riding_now.is_some() => {
                commands.entity(avatar).remove::<AvatarSeated>();
            }
            _ => {}
        }
    }
}

/// Bind a character's `HumanoidRootPart` to the avatar standing for it, as
/// the host binds its own: the Animator finds the skeleton through it, and a
/// raycast or touch on the avatar resolves to the character. A part the tree
/// drew for the root before `Character` pointed at it goes; a binding the
/// avatar had to an older root (the character before a respawn) moves here.
/// The tree's apply step never despawns or unbinds an entity it did not
/// draw, so the binding is this system's to keep.
fn bind_root(commands: &mut Commands, g: &mut DataModel, root: InstanceId, avatar: Entity, drawn: &Query<(), With<FromTree>>) {
    let bits = avatar.to_bits();
    match g.entity_of(root) {
        Some(b) if b == bits => return,
        Some(b) => {
            let old = Entity::from_bits(b);
            if drawn.contains(old) {
                commands.entity(old).try_despawn();
            }
            g.unbind_entity(root);
        }
        None => {}
    }
    if let Some(previous) = g.by_entity(bits).filter(|p| *p != root && g.entity_of(*p) == Some(bits)) {
        g.unbind_entity(previous);
    }
    g.bind_entity(root, bits);
}

/// This Player's `FireServer` and `InvokeServer` calls, to the host.
fn send_remote_calls(tree: Option<Res<PlayDataModel>>, state: Res<PlayerReplica>, mut fire: MessageWriter<FireRemote>) {
    let (Some(tree), Some(replica)) = (tree, state.replica.as_ref()) else { return };
    let mut g = tree.dm.lock();
    for call in std::mem::take(&mut g.remote_out) {
        if !matches!(call.target, RemoteTarget::Server) {
            continue;
        }
        let Some(remote) = replica.net_of(call.remote) else {
            debug!("net: {} is not the host's, so its call stays here", g.full_name(call.remote));
            continue;
        };
        let unreliable = g.class_of(call.remote) == Some("UnreliableRemoteEvent");
        let args = call.args.iter().map(|a| to_wire(a, &|id| replica.net_of(id))).collect();
        let number = call.invocation.map_or(0, |i| i as u32);
        fire.write(FireRemote {
            call: eustress_networking::repl::RemoteCall { remote, args, call: number },
            unreliable,
        });
    }
}

/// The host's answers to this Player's `InvokeServer` calls.
fn receive_replies(tree: Option<Res<PlayDataModel>>, state: Res<PlayerReplica>, mut replies: MessageReader<RemoteReplied>) {
    let (Some(tree), Some(replica)) = (tree, state.replica.as_ref()) else {
        replies.clear();
        return;
    };
    let mut g = tree.dm.lock();
    for RemoteReplied(reply) in replies.read() {
        let result = reply
            .result
            .as_ref()
            .map(|values| values.iter().map(|v| from_wire(v, &|n| replica.local_of(n))).collect())
            .map_err(|e| e.clone());
        g.reply_in.push(RemoteReply { invocation: reply.call as u64, to: None, result });
    }
}

fn to_wire(v: &RemoteValue, net_of: &dyn Fn(InstanceId) -> Option<NetId>) -> WireValue {
    match v {
        RemoteValue::Value(d) => WireValue::from_dm(d, net_of),
        RemoteValue::Array(items) => WireValue::Array(items.iter().map(|i| to_wire(i, net_of)).collect()),
        RemoteValue::Map(entries) => WireValue::Map(entries.iter().map(|(k, v)| (k.clone(), to_wire(v, net_of))).collect()),
    }
}

fn from_wire(v: &WireValue, local_of: &dyn Fn(NetId) -> Option<InstanceId>) -> RemoteValue {
    match v {
        WireValue::Array(items) => RemoteValue::Array(items.iter().map(|i| from_wire(i, local_of)).collect()),
        WireValue::Map(entries) => RemoteValue::Map(entries.iter().map(|(k, v)| (k.clone(), from_wire(v, local_of))).collect()),
        other => RemoteValue::Value(other.to_dm(local_of).unwrap_or(DmValue::Nil)),
    }
}
