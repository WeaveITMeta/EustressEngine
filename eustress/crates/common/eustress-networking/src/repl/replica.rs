//! The player half: apply the host's world frames to this player's tree.
//!
//! Every change goes through the tree's own writes (`create`, `set_prop`,
//! `set_parent`, `destroy`), so it marks itself for the Player's apply step
//! to render, and `Changed`, `ChildAdded` and `Destroying` fire for
//! LocalScripts exactly as they would for a local change.

use std::collections::HashMap;

use eustress_common::datamodel::{
    is_service_class, DataModel, DmEvent, DmValue, InstanceId, SoundAction, SoundCommand,
};

use super::id::{scene_net_id, service_net_id, NetId, NetIdMap, TERRAIN_KEY};
use super::ops::{ReplOp, SpawnOp, WorldFrame};
use super::tracks::{ClockLink, PlayerTracks, TrackWire};
use super::value::{Unresolved, WireValue};
use crate::wire::PeerId;

/// Frames an unresolved instance reference is retried for before it is
/// dropped (it named something this player will never see).
const REF_PATIENCE: u32 = 64;

struct PendingRef {
    on: InstanceId,
    prop: String,
    wants: NetId,
    age: u32,
}

/// What a frame did that the tree does not hold.
#[derive(Debug, Default)]
pub struct Applied {
    /// RemoteEvents the host fired at this player, in order: the remote, and
    /// its arguments (tables intact) for the script runtime to deliver.
    pub remotes: Vec<(InstanceId, Vec<WireValue>)>,
    /// Ops about instances this player does not have.
    pub unknown: usize,
    /// Writes the tree refused, with the reason.
    pub problems: Vec<String>,
}

/// A player's replication state for one session.
#[derive(Default)]
pub struct Replica {
    ids: NetIdMap,
    me: Option<(PeerId, InstanceId)>,
    /// Every player's `Player` instance, by session peer.
    players: HashMap<PeerId, InstanceId>,
    pending_refs: Vec<PendingRef>,
    last_tick: u64,
    tracks: PlayerTracks,
    /// This tree's clock against the host's, for track times.
    clock: Option<ClockLink>,
}

impl Replica {
    /// Bound to `game`, every service `dm` has, and `workspace.Terrain` when
    /// the tree has one.
    pub fn new(dm: &DataModel) -> Self {
        let mut r = Self::default();
        r.ids.bind(NetId::ROOT, dm.root());
        for &child in dm.children(dm.root()) {
            if let Some(class) = dm.class_of(child).filter(|c| is_service_class(c)) {
                r.ids.bind(service_net_id(class), child);
            }
        }
        if let Some(terrain) = dm.find_service("Workspace").and_then(|ws| dm.find_first_child_of_class(ws, "Terrain", false)) {
            r.ids.bind(scene_net_id(TERRAIN_KEY), terrain);
        }
        r
    }

    /// A scene instance this player loaded: `record_key` is the
    /// Space-relative path of the file that defined it.
    pub fn bind_scene(&mut self, record_key: &str, id: InstanceId) -> NetId {
        let net = scene_net_id(record_key);
        self.ids.bind(net, id);
        net
    }

    /// This player's own `Player` instance and session peer, so the host's
    /// `Player` for this peer lands on it instead of making a second one.
    pub fn set_local_player(&mut self, peer: PeerId, player: InstanceId) {
        self.me = Some((peer, player));
        self.players.insert(peer, player);
    }

    /// The `Player` instance of session peer `peer`: this player's own, the
    /// host's (peer 0), or another joined player's.
    pub fn player_of(&self, peer: PeerId) -> Option<InstanceId> {
        self.players.get(&peer).copied()
    }

    /// The session peer a `Player` instance stands for.
    pub fn peer_of(&self, player: InstanceId) -> Option<PeerId> {
        self.players.iter().find(|(_, p)| **p == player).map(|(peer, _)| *peer)
    }

    pub fn local_of(&self, net: NetId) -> Option<InstanceId> {
        self.ids.local(net)
    }

    pub fn net_of(&self, id: InstanceId) -> Option<NetId> {
        self.ids.net(id)
    }

    /// The newest host tick applied.
    pub fn tick(&self) -> u64 {
        self.last_tick
    }

    /// Where this tree's `frame.time` stands against the host's tick now.
    /// Set before applying frames, for the track changes they carry.
    pub fn set_clock(&mut self, clock: Option<ClockLink>) {
        self.clock = clock;
    }

    /// This player's scripts' track changes on its own character, for the
    /// host (`ToHost::Tracks`).
    pub fn outgoing_tracks(&mut self, dm: &mut DataModel, clock: ClockLink) -> Vec<TrackWire> {
        let Replica { tracks, ids, .. } = self;
        tracks.outgoing(dm, clock, &|id| ids.net(id))
    }

    /// Apply one frame, in order.
    pub fn apply(&mut self, dm: &mut DataModel, frame: &WorldFrame) -> Applied {
        let mut applied = Applied::default();
        self.last_tick = self.last_tick.max(frame.tick);
        for op in &frame.ops {
            self.apply_op(dm, op, &mut applied);
        }
        self.retry_refs(dm);
        applied
    }

    fn apply_op(&mut self, dm: &mut DataModel, op: &ReplOp, applied: &mut Applied) {
        if let ReplOp::Spawn(s) = op {
            return self.spawn(dm, s, applied);
        }
        if let ReplOp::Track { from, op } = op {
            let me = self.me.map(|(peer, _)| peer);
            let Replica { tracks, ids, clock, .. } = self;
            if let Err(e) = tracks.apply(dm, me, *from, op, *clock, &|n| ids.local(n)) {
                applied.problems.push(e);
            }
            return;
        }
        let Some(local) = self.ids.local(op.subject()).filter(|id| dm.exists(*id)) else {
            applied.unknown += 1;
            return;
        };
        match op {
            ReplOp::Spawn(_) | ReplOp::Track { .. } => unreachable!("handled above"),
            ReplOp::SetProps { props, .. } => self.write_props(dm, local, props, applied),
            ReplOp::SetAttributes { attributes, .. } => {
                for (name, v) in attributes {
                    match v.to_dm(&|n| self.ids.local(n)) {
                        Ok(value) => {
                            if let Err(e) = dm.set_attribute(local, name, value) {
                                applied.problems.push(format!("attribute {name}: {e}"));
                            }
                        }
                        Err(_) => applied.problems.push(format!("attribute {name}: not an attribute value")),
                    }
                }
            }
            ReplOp::SetTags { tags, .. } => {
                for t in dm.tags_of(local) {
                    if !tags.contains(&t) {
                        dm.remove_tag(local, &t);
                    }
                }
                for t in tags {
                    dm.add_tag(local, t);
                }
            }
            ReplOp::Reparent { parent, .. } => {
                let target = if parent.is_none() {
                    None
                } else {
                    match self.ids.local(*parent) {
                        Some(p) => Some(p),
                        None => {
                            applied.unknown += 1;
                            return;
                        }
                    }
                };
                if let Err(e) = dm.set_parent(local, target) {
                    applied.problems.push(format!("reparent {}: {e}", dm.full_name(local)));
                }
            }
            ReplOp::Destroy { .. } => {
                if Some(local) == self.me.map(|(_, p)| p) || local == dm.root() {
                    return; // the host never takes this player's own Player or `game`
                }
                // A character going away: its player's scripts hear it while
                // it still exists, as the host's did.
                for player in self.owners_of(dm, local) {
                    dm.push_event(DmEvent::CharacterRemoving { player, character: local });
                }
                // Another player left: this Player's scripts hear it first.
                if dm.class_of(local) == Some("Player") && dm.parent(local).is_some() {
                    dm.push_event(DmEvent::PlayerRemoving { player: local });
                }
                self.players.retain(|_, p| *p != local);
                self.forget(dm, local);
                dm.destroy(local);
            }
            ReplOp::Remote { args, .. } => applied.remotes.push((local, args.clone())),
            ReplOp::Sound { action, .. } => {
                let action = match action {
                    0 => SoundAction::Play,
                    1 => SoundAction::Stop,
                    2 => SoundAction::Pause,
                    3 => SoundAction::Resume,
                    other => {
                        applied.problems.push(format!("sound action {other} is not one"));
                        return;
                    }
                };
                dm.sound_commands.push(SoundCommand { sound: local, action });
            }
            ReplOp::Emit { count, .. } => dm.particle_emits.push((local, (*count).min(10_000))),
        }
    }

    fn spawn(&mut self, dm: &mut DataModel, s: &SpawnOp, applied: &mut Applied) {
        // The host's Player for this peer is this player's own.
        let own = match self.me {
            Some((peer, player)) if s.class == "Player" && s.peer == Some(peer) && dm.exists(player) => Some(player),
            _ => None,
        };
        let id = match own {
            Some(player) => player,
            None => {
                // A spawn of an id this player has replaces it: the host's
                // state wins.
                if let Some(old) = self.ids.local(s.id).filter(|id| dm.exists(*id)) {
                    self.forget(dm, old);
                    dm.destroy(old);
                }
                dm.create(&s.class)
            }
        };
        self.ids.bind(s.id, id);
        if let (true, Some(peer)) = (s.class == "Player", s.peer) {
            self.players.insert(peer, id);
        }
        if let Err(e) = dm.rename(id, &s.name) {
            applied.problems.push(format!("name of {}: {e}", s.class));
        }
        // A player joins before its character arrives, as on the host: a
        // `Player`'s `Character` is written once it is in the tree.
        let (character, props): (Vec<_>, Vec<_>) =
            s.props.iter().cloned().partition(|(name, _)| s.class == "Player" && name == "Character");
        self.write_props(dm, id, &props, applied);
        for (name, v) in &s.attributes {
            if let Ok(value) = v.to_dm(&|n| self.ids.local(n)) {
                let _ = dm.set_attribute(id, name, value);
            }
        }
        for t in &s.tags {
            dm.add_tag(id, t);
        }
        // Last, so the instance enters the tree complete and the apply step
        // builds it from its final properties.
        if !s.parent.is_none() {
            match self.ids.local(s.parent) {
                Some(p) => {
                    if dm.parent(id) != Some(p) {
                        if let Err(e) = dm.set_parent(id, Some(p)) {
                            applied.problems.push(format!("parent of {}: {e}", s.name));
                        } else if own.is_none() && s.class == "Player" && dm.class_of(p) == Some("Players") {
                            // Another player is here, as `Players.PlayerAdded` says
                            // on a Roblox client. This Player's own `Player` is
                            // `LocalPlayer`, which scripts read directly.
                            dm.push_event(DmEvent::PlayerAdded { player: id });
                        }
                    }
                }
                None => applied.unknown += 1,
            }
        }
        self.write_props(dm, id, &character, applied);
    }

    fn write_props(&mut self, dm: &mut DataModel, id: InstanceId, props: &[(String, WireValue)], applied: &mut Applied) {
        for (name, v) in props {
            if name == "Name" {
                if let WireValue::String(n) = v {
                    let _ = dm.rename(id, n);
                }
                continue;
            }
            match v.to_dm(&|n| self.ids.local(n)) {
                Ok(value) => {
                    if let Err(e) = write(dm, id, name, value) {
                        applied.problems.push(format!("{name}: {e}"));
                    }
                }
                Err(Unresolved::Instance(wants)) => {
                    self.pending_refs.push(PendingRef { on: id, prop: name.clone(), wants, age: 0 })
                }
                Err(Unresolved::Table) => applied.problems.push(format!("{name}: a table is not a property value")),
            }
        }
    }

    /// References to instances that had not arrived: set them once they do.
    fn retry_refs(&mut self, dm: &mut DataModel) {
        let pending = std::mem::take(&mut self.pending_refs);
        for mut r in pending {
            if !dm.exists(r.on) {
                continue;
            }
            match self.ids.local(r.wants) {
                Some(target) => {
                    let _ = write(dm, r.on, &r.prop, DmValue::Instance(target));
                }
                None if r.age < REF_PATIENCE => {
                    r.age += 1;
                    self.pending_refs.push(r);
                }
                None => {}
            }
        }
    }

    /// Unbind `id` and everything under it.
    fn forget(&mut self, dm: &DataModel, id: InstanceId) {
        for node in std::iter::once(id).chain(dm.descendants(id)) {
            self.ids.unbind_local(node);
        }
    }

    /// The players whose `Character` is `model`.
    fn owners_of(&self, dm: &DataModel, model: InstanceId) -> Vec<InstanceId> {
        self.players
            .values()
            .copied()
            .filter(|&p| dm.get_prop(p, "Character").and_then(|v| v.as_instance()) == Some(model))
            .collect()
    }
}

/// A property write, announcing a `Player`'s new `Character` as the host's
/// engine does: `CharacterRemoving` for the one it had while that still
/// exists, then `CharacterAdded`.
fn write(dm: &mut DataModel, id: InstanceId, name: &str, value: DmValue) -> Result<(), String> {
    if name != "Character" || dm.class_of(id) != Some("Player") {
        return dm.set_prop(id, name, value);
    }
    let old = dm.get_prop(id, "Character").and_then(|v| v.as_instance());
    let new = value.as_instance();
    if old == new {
        return dm.set_prop(id, name, value);
    }
    if let Some(old) = old.filter(|m| dm.exists(*m)) {
        dm.push_event(DmEvent::CharacterRemoving { player: id, character: old });
    }
    dm.set_prop(id, name, value)?;
    if let Some(new) = new {
        dm.push_event(DmEvent::CharacterAdded { player: id, character: new });
    }
    Ok(())
}
