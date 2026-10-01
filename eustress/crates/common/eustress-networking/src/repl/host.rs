//! The host half: what the host's tree did, as ops for each player.
//!
//! ```text
//! Play starts     HostReplicator::bind_tree   every instance gets its NetId;
//!                                             what players loaded is noted
//! every frame     HostReplicator::observe     events + dirty properties +
//!                                             emits -> ops, by audience
//! a player joins  HostReplicator::snapshot    everything since the world was
//!                                             exported, for that player
//! ```
//!
//! The host reads the DataModel's event log for structure (every move into,
//! out of, and within the replicated containers, every destroy, and the
//! joined players and characters the engine announces) and the property
//! names the engine's apply step drained (through [`ReplicationTap`]) for
//! writes. Poses physics produces are not writes; the motion lane carries
//! those.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use bevy::prelude::Resource;
use eustress_common::datamodel::{is_service_class, DataModel, DmEvent, InstanceId};

use super::id::{scene_net_id, service_net_id, NetId, NetIdMap, TERRAIN_KEY};
use super::ops::{audience, replicates_prop, Audience, ReplOp, SpawnOp, WorldFrame};
use super::value::WireValue;
use crate::wire::PeerId;

/// Property writes the engine's apply step drained this frame, kept for the
/// replicator. The host shell inserts it while a session is hosted; the apply
/// step appends to it when it exists, and the replicator empties it.
#[derive(Resource, Default, Debug)]
pub struct ReplicationTap {
    pub dirty: Vec<(InstanceId, Vec<String>)>,
}

/// What a player loads with the world: each scene instance's id, by its
/// record key, with its parent's id. Built from the exported records the way
/// the Player reads them (`tree_read`), so it names exactly what a joiner has.
#[derive(Debug, Clone, Default)]
pub struct WorldLayout {
    parents: HashMap<NetId, NetId>,
}

impl WorldLayout {
    /// One Space's records as players receive them.
    pub fn of_records(records: &[(String, Vec<u8>)]) -> WorldLayout {
        let scene = eustress_common::tree_read::read_space_records(records);
        Self::of_tree(&scene.dm, &scene.keys)
    }

    /// A tree read from those records, with its record keys.
    pub fn of_tree(dm: &DataModel, keys: &[(String, InstanceId)]) -> WorldLayout {
        let key_of: HashMap<InstanceId, String> = keys.iter().map(|(k, id)| (*id, k.clone())).collect();
        let mut r = HostReplicator::new(dm);
        r.bind_tree(dm, &|id| key_of.get(&id).cloned());
        let parents = keys
            .iter()
            .filter_map(|(key, _)| {
                let net = scene_net_id(key);
                r.parent_on_players.get(&net).map(|parent| (net, *parent))
            })
            .collect();
        WorldLayout { parents }
    }

    pub fn len(&self) -> usize {
        self.parents.len()
    }
}

/// What a Play session wrote before it was hosted: the names of the
/// properties, attributes and tags scripts changed, per instance. Names only,
/// so it stays as small as the tree however long Play runs; values are read
/// when a player joins.
#[derive(Debug, Default)]
pub struct PreHostRecord {
    cursor: u64,
    props: HashMap<InstanceId, BTreeSet<String>>,
    attrs: HashMap<InstanceId, BTreeSet<String>>,
    tags: HashSet<InstanceId>,
}

impl PreHostRecord {
    /// Property writes the apply step drained ([`ReplicationTap`]).
    pub fn fold_writes(&mut self, dirty: &[(InstanceId, Vec<String>)]) {
        for (id, names) in dirty {
            self.props.entry(*id).or_default().extend(names.iter().cloned());
        }
    }

    /// The tree's attribute and tag changes since the last call.
    pub fn fold_events(&mut self, dm: &DataModel) {
        let (events, next) = dm.events_since(self.cursor);
        self.cursor = next;
        for e in events {
            match e {
                DmEvent::AttributeChanged { id, name } => {
                    self.attrs.entry(id).or_default().insert(name);
                }
                DmEvent::TagAdded { id, .. } | DmEvent::TagRemoved { id, .. } => {
                    self.tags.insert(id);
                }
                _ => {}
            }
        }
    }
}

/// One frame's worth of script effects besides the tree itself.
#[derive(Default)]
pub struct FrameEffects<'a> {
    pub emits: &'a [(InstanceId, u32)],
}

/// Ops for players, each with who may receive it.
pub type Outgoing = Vec<(Audience, ReplOp)>;

/// The host's replication state for one Play session.
#[derive(Default)]
pub struct HostReplicator {
    ids: NetIdMap,
    next_runtime: u64,
    /// Every instance players have now, and who has it.
    sent: HashMap<NetId, Audience>,
    /// Where each of those sits on players.
    parent_on_players: HashMap<NetId, NetId>,
    /// Instances players did not load with the world: runtime ones, and
    /// scene ones that were out of view when it was exported.
    absent_from_world: HashSet<NetId>,
    // What late joiners need beyond the world, per scene instance.
    scene_removed: BTreeSet<NetId>,
    scene_moved: BTreeSet<NetId>,
    scene_props: BTreeMap<NetId, BTreeSet<String>>,
    scene_attrs: BTreeMap<NetId, BTreeSet<String>>,
    scene_tags: BTreeSet<NetId>,
    moved_by_physics: BTreeSet<NetId>,
    /// Each joined player's `Player` instance, and its session peer.
    players: HashMap<InstanceId, PeerId>,
    cursor: u64,
    tick: u64,
    /// Two instances whose record keys hashed alike: the second is left
    /// out of replication, with the reason here.
    pub problems: Vec<String>,
}

impl HostReplicator {
    /// Start at `dm`'s current state. Call [`Self::bind_tree`] next.
    pub fn new(dm: &DataModel) -> Self {
        let mut r = Self { cursor: dm.event_cursor(), ..Default::default() };
        r.ids.bind(NetId::ROOT, dm.root());
        r.sent.insert(NetId::ROOT, Audience::Everyone);
        r
    }

    /// Give every instance in the tree its id. `key_of` names the record an
    /// instance was loaded from (its Space-relative file path), or `None` for
    /// one made at run time. Instances in view now are what players loaded
    /// with the world, except runtime ones, which a joining player is sent.
    pub fn bind_tree(&mut self, dm: &DataModel, key_of: &dyn Fn(InstanceId) -> Option<String>) {
        let root = dm.root();
        let mut stack: Vec<InstanceId> = dm.children(root).iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            stack.extend(dm.children(id).iter().rev().copied());
            let Some(inst) = dm.get(id) else { continue };
            let seen = audience(dm, id);
            let net = if inst.parent == Some(root) && is_service_class(&inst.class_name) {
                Some(service_net_id(&inst.class_name))
            } else if let Some(key) = key_of(id) {
                Some(scene_net_id(&key))
            } else if inst.class_name == "Terrain" && inst.parent.and_then(|p| dm.class_of(p)) == Some("Workspace") {
                Some(scene_net_id(TERRAIN_KEY))
            } else {
                None
            };
            let net = match net {
                Some(n) => match self.ids.local(n) {
                    Some(other) if other != id => {
                        self.problems.push(format!(
                            "{} and {} have the same replication id; {} is not replicated",
                            dm.full_name(other),
                            dm.full_name(id),
                            dm.full_name(id)
                        ));
                        continue;
                    }
                    _ => n,
                },
                // Made at run time (the host's own Player, its character):
                // players get it when they join, so it is minted now.
                None if seen.is_some() => self.mint(),
                None => continue,
            };
            self.ids.bind(net, id);
            if net.is_runtime() || seen.is_none() {
                self.absent_from_world.insert(net);
            }
            if let Some(a) = seen {
                self.sent.insert(net, a);
            }
        }
        // Parents, now that every id exists.
        let sent: Vec<NetId> = self.sent.keys().copied().collect();
        for net in sent {
            let parent = self.ids.local(net).and_then(|id| dm.parent(id));
            let p = parent.and_then(|p| self.ids.net(p)).unwrap_or(NetId::NONE);
            self.parent_on_players.insert(net, p);
        }
    }

    /// A joined player's `Player` instance.
    pub fn set_player(&mut self, player: InstanceId, peer: PeerId) {
        self.players.insert(player, peer);
    }

    pub fn remove_player(&mut self, player: InstanceId) {
        self.players.remove(&player);
    }

    /// The host tick stamped on the next frame.
    pub fn set_tick(&mut self, tick: u64) {
        self.tick = tick;
    }

    pub fn net_of(&self, id: InstanceId) -> Option<NetId> {
        self.ids.net(id)
    }

    pub fn local_of(&self, net: NetId) -> Option<InstanceId> {
        self.ids.local(net)
    }

    /// Whether `player` (its `Player` instance) has `net`.
    pub fn visible_to(&self, net: NetId, player: InstanceId) -> bool {
        match self.sent.get(&net) {
            Some(Audience::Everyone) => true,
            Some(Audience::Owner(p)) => *p == player,
            None => false,
        }
    }

    /// Whether everyone `audience` names has `net`: what a call aimed at
    /// that audience may name.
    pub fn seen_by(&self, net: NetId, audience: Audience) -> bool {
        match (self.sent.get(&net), audience) {
            (Some(Audience::Everyone), _) => true,
            (Some(Audience::Owner(p)), Audience::Owner(q)) => *p == q,
            _ => false,
        }
    }

    /// The id to put on a body's motion, when every player has it.
    pub fn motion_id(&self, id: InstanceId) -> Option<NetId> {
        let net = self.ids.net(id)?;
        (self.sent.get(&net) == Some(&Audience::Everyone)).then_some(net)
    }

    /// Physics moved `net`: a player joining later must be told where it is.
    pub fn note_motion(&mut self, net: NetId) {
        if !self.absent_from_world.contains(&net) {
            self.moved_by_physics.insert(net);
        }
    }

    fn mint(&mut self) -> NetId {
        self.next_runtime += 1;
        NetId::runtime(self.next_runtime)
    }

    fn wire(&self, v: &eustress_common::datamodel::DmValue) -> WireValue {
        WireValue::from_dm(v, &|id| self.ids.net(id).filter(|n| self.sent.contains_key(n)))
    }

    fn in_world(&self, net: NetId) -> bool {
        net.is_scene() && !self.absent_from_world.contains(&net) && !self.scene_removed.contains(&net)
    }

    /// What changed this frame, as ops. `dirty` is what the apply step
    /// drained (see [`ReplicationTap`]).
    pub fn observe(&mut self, dm: &DataModel, dirty: &[(InstanceId, Vec<String>)], effects: FrameEffects<'_>) -> Outgoing {
        let (events, next) = dm.events_since(self.cursor);
        self.cursor = next;
        // A player who left takes its peer with it.
        self.players.retain(|id, _| dm.exists(*id));

        let mut out: Outgoing = Vec::new();
        let mut touched: Vec<InstanceId> = Vec::new();
        let mut seen: HashSet<InstanceId> = HashSet::new();
        let mut attrs: BTreeMap<InstanceId, BTreeSet<String>> = BTreeMap::new();
        let mut tags: BTreeSet<InstanceId> = BTreeSet::new();
        let mut characters: BTreeSet<InstanceId> = BTreeSet::new();
        for e in &events {
            match e {
                DmEvent::AncestryChanged { id, .. } | DmEvent::Destroying { id } => {
                    if seen.insert(*id) {
                        touched.push(*id);
                    }
                }
                DmEvent::AttributeChanged { id, name } => {
                    attrs.entry(*id).or_default().insert(name.clone());
                }
                DmEvent::TagAdded { id, .. } | DmEvent::TagRemoved { id, .. } => {
                    tags.insert(*id);
                }
                // The engine builds a joined player's `Player` and every
                // character already parented (`create_virtual`), which fires
                // no structure event, and announces them with these instead.
                DmEvent::PlayerAdded { player } => {
                    if seen.insert(*player) {
                        touched.push(*player);
                    }
                }
                DmEvent::CharacterAdded { player, character } => {
                    if seen.insert(*character) {
                        touched.push(*character);
                    }
                    characters.insert(*player);
                }
                DmEvent::CharacterRemoving { player, .. } => {
                    characters.insert(*player);
                }
                _ => {}
            }
        }

        // Ancestors before descendants, so a model reaches players before
        // the parts a script put in it before parenting it.
        let depth = |id: InstanceId| {
            let mut d = 0usize;
            let mut cur = dm.parent(id);
            while let Some(p) = cur {
                d += 1;
                cur = dm.parent(p);
            }
            d
        };
        touched.sort_by_cached_key(|id| depth(*id));

        let mut spawned_now: HashSet<NetId> = HashSet::new();
        for id in touched {
            let net = self.ids.net(id);
            let was = net.and_then(|n| self.sent.get(&n).copied());
            let now = audience(dm, id);
            match (was, now) {
                (None, None) => {}
                (None, Some(_)) => self.spawn_subtree(dm, id, &mut out, &mut spawned_now),
                (Some(a), None) => {
                    let net = net.expect("sent implies bound");
                    // Inside a subtree leaving at the same time: its root's
                    // Destroy covers it.
                    let parent_leaving = dm.parent(id).is_some_and(|p| {
                        self.ids.net(p).is_some_and(|pn| self.sent.contains_key(&pn)) && audience(dm, p).is_none()
                    });
                    if !parent_leaving {
                        out.push((a, ReplOp::Destroy { id: net }));
                    }
                    let destroyed = dm.get(id).map_or(true, |i| i.destroyed);
                    self.forget_subtree(dm, id, destroyed);
                }
                (Some(a), Some(b)) if a == b => {
                    let net = net.expect("sent implies bound");
                    let parent = dm.parent(id).and_then(|p| self.ids.net(p)).unwrap_or(NetId::NONE);
                    if self.parent_on_players.get(&net) != Some(&parent) {
                        out.push((a, ReplOp::Reparent { id: net, parent }));
                        self.parent_on_players.insert(net, parent);
                        if net.is_scene() {
                            self.scene_moved.insert(net);
                        }
                    }
                }
                (Some(a), Some(_)) => {
                    // Moved between a public container and a private one.
                    let net = net.expect("sent implies bound");
                    out.push((a, ReplOp::Destroy { id: net }));
                    self.forget_subtree(dm, id, false);
                    self.spawn_subtree(dm, id, &mut out, &mut spawned_now);
                }
            }
        }

        // Attributes and tags, from their events.
        for (id, names) in attrs {
            let Some((net, a)) = self.live(id) else { continue };
            if spawned_now.contains(&net) {
                continue;
            }
            let attributes: Vec<(String, WireValue)> = names
                .iter()
                .map(|n| (n.clone(), dm.get_attribute(id, n).map(|v| self.wire(&v)).unwrap_or(WireValue::Nil)))
                .collect();
            if net.is_scene() {
                self.scene_attrs.entry(net).or_default().extend(names);
            }
            out.push((a, ReplOp::SetAttributes { id: net, attributes }));
        }
        for id in tags {
            let Some((net, a)) = self.live(id) else { continue };
            if spawned_now.contains(&net) {
                continue;
            }
            if net.is_scene() {
                self.scene_tags.insert(net);
            }
            out.push((a, ReplOp::SetTags { id: net, tags: dm.tags_of(id) }));
        }

        // Property writes, with the values they have now.
        for (id, names) in dirty {
            let Some((net, a)) = self.live(*id) else { continue };
            if spawned_now.contains(&net) {
                continue;
            }
            let class = dm.class_of(*id).unwrap_or_default();
            let mut props: Vec<(String, WireValue)> = Vec::new();
            for name in names {
                if !replicates_prop(class, name) || props.iter().any(|(n, _)| n == name) {
                    continue;
                }
                let value = dm.get_prop(*id, name).unwrap_or_default();
                props.push((name.clone(), self.wire(&value)));
            }
            if props.is_empty() {
                continue;
            }
            if net.is_scene() {
                self.scene_props.entry(net).or_default().extend(props.iter().map(|(n, _)| n.clone()));
            }
            out.push((a, ReplOp::SetProps { id: net, props }));
        }
        // A `Player`'s `Character` is set by the engine, never drained as a
        // script write: its events stand for the write. It goes after the
        // structure above, so the model it names is on players already. A
        // player spawned this frame is included: its spawn may have come
        // before its character's model did.
        for player in characters {
            let Some((net, a)) = self.live(player) else { continue };
            let written = !spawned_now.contains(&net)
                && dirty.iter().any(|(id, names)| *id == player && names.iter().any(|n| n == "Character"));
            if written {
                continue;
            }
            let value = dm.get_prop(player, "Character").unwrap_or_default();
            out.push((a, ReplOp::SetProps { id: net, props: vec![("Character".into(), self.wire(&value))] }));
        }

        for (emitter, count) in effects.emits {
            if let Some((net, a)) = self.live(*emitter) {
                out.push((a, ReplOp::Emit { emitter: net, count: *count }));
            }
        }
        out
    }

    /// An instance players have now, with its id and who has it.
    pub fn live_of(&self, id: InstanceId) -> Option<(NetId, Audience)> {
        self.live(id)
    }

    /// An instance players have now, with its id and audience.
    fn live(&self, id: InstanceId) -> Option<(NetId, Audience)> {
        let net = self.ids.net(id)?;
        Some((net, *self.sent.get(&net)?))
    }

    /// Hosting began in a Play session that was already running: players
    /// load the world as exported (`world`), and this tree differs from it by
    /// everything the session did before now. Record that, so every late
    /// joiner's snapshot brings it: scene instances gone, or now out of
    /// players' view, are removed; ones under another parent move; ones
    /// players lack are sent whole; and the properties, attributes and tags
    /// scripts wrote (`record`) go with their current values. Call right
    /// after [`Self::bind_tree`], with the same `key_of`. Without a `world`
    /// only the writes are recorded.
    pub fn seed_before_hosting(
        &mut self,
        dm: &DataModel,
        world: Option<&WorldLayout>,
        key_of: &dyn Fn(InstanceId) -> Option<String>,
        record: &PreHostRecord,
    ) {
        if let Some(world) = world {
            let mut here: HashSet<NetId> = HashSet::new();
            for id in dm.descendants(dm.root()) {
                let Some(key) = key_of(id) else { continue };
                let net = scene_net_id(&key);
                if self.ids.local(net) != Some(id) {
                    continue;
                }
                here.insert(net);
                match (self.sent.contains_key(&net), world.parents.get(&net)) {
                    (true, Some(parent)) => {
                        if self.parent_on_players.get(&net) != Some(parent) {
                            self.scene_moved.insert(net);
                        }
                    }
                    // Players never loaded it: a joiner is sent it whole.
                    (true, None) => {
                        self.absent_from_world.insert(net);
                    }
                    // Players loaded it, and it is out of their view now.
                    (false, Some(_)) => {
                        self.scene_removed.insert(net);
                    }
                    (false, None) => {}
                }
            }
            for net in world.parents.keys() {
                if !here.contains(net) {
                    self.scene_removed.insert(*net);
                }
            }
        }
        for (id, names) in &record.props {
            let Some((net, _)) = self.live(*id).filter(|(n, _)| n.is_scene()) else { continue };
            let class = dm.class_of(*id).unwrap_or_default();
            let names: Vec<String> = names.iter().filter(|n| replicates_prop(class, n)).cloned().collect();
            if !names.is_empty() {
                self.scene_props.entry(net).or_default().extend(names);
            }
        }
        for (id, names) in &record.attrs {
            let Some((net, _)) = self.live(*id).filter(|(n, _)| n.is_scene()) else { continue };
            self.scene_attrs.entry(net).or_default().extend(names.iter().cloned());
        }
        for id in &record.tags {
            if let Some((net, _)) = self.live(*id).filter(|(n, _)| n.is_scene()) {
                self.scene_tags.insert(net);
            }
        }
    }

    /// Send `id` and everything under it players may see, parents first.
    /// Ids come first, so a property naming a later sibling (a model's
    /// `PrimaryPart`) already has one when the model is encoded.
    fn spawn_subtree(&mut self, dm: &DataModel, id: InstanceId, out: &mut Outgoing, spawned_now: &mut HashSet<NetId>) {
        let mut order: Vec<(InstanceId, NetId, Audience)> = Vec::new();
        let mut stack = vec![id];
        while let Some(node) = stack.pop() {
            let Some(a) = audience(dm, node) else { continue };
            let net = match self.ids.net(node) {
                Some(n) => n,
                None => {
                    let n = self.mint();
                    self.ids.bind(n, node);
                    self.absent_from_world.insert(n);
                    n
                }
            };
            match self.sent.get(&net).copied() {
                Some(already) if already == a => {
                    // Players have it (and what is under it): it only moved.
                    let parent = dm.parent(node).and_then(|p| self.ids.net(p)).unwrap_or(NetId::NONE);
                    if self.parent_on_players.get(&net) != Some(&parent) {
                        out.push((a, ReplOp::Reparent { id: net, parent }));
                        self.parent_on_players.insert(net, parent);
                        if net.is_scene() {
                            self.scene_moved.insert(net);
                        }
                    }
                    continue;
                }
                // Seen by someone else until now: theirs goes, this one comes.
                Some(already) => out.push((already, ReplOp::Destroy { id: net })),
                None => {}
            }
            self.sent.insert(net, a);
            order.push((node, net, a));
            stack.extend(dm.children(node).iter().rev().copied());
        }
        for (node, net, a) in order {
            let Some(op) = self.spawn_op(dm, node, net) else { continue };
            self.parent_on_players.insert(net, op.parent);
            spawned_now.insert(net);
            out.push((a, ReplOp::Spawn(op)));
        }
    }

    fn spawn_op(&self, dm: &DataModel, node: InstanceId, net: NetId) -> Option<SpawnOp> {
        let inst = dm.get(node).filter(|i| !i.destroyed)?;
        let mut props: Vec<(String, WireValue)> = inst
            .props
            .iter()
            .filter(|(k, _)| replicates_prop(&inst.class_name, k))
            .map(|(k, v)| (k.clone(), self.wire(v)))
            .collect();
        props.sort_by(|a, b| a.0.cmp(&b.0));
        Some(SpawnOp {
            id: net,
            class: inst.class_name.clone(),
            name: inst.name.clone(),
            parent: inst.parent.and_then(|p| self.ids.net(p)).unwrap_or(NetId::NONE),
            props,
            attributes: inst.attributes.iter().map(|(k, v)| (k.clone(), self.wire(v))).collect(),
            tags: inst.tags.iter().cloned().collect(),
            peer: self.players.get(&node).copied(),
        })
    }

    /// Players no longer have `id` or anything under it. A destroyed subtree
    /// also gives up its ids; one that only left view keeps them, so it comes
    /// back as itself.
    fn forget_subtree(&mut self, dm: &DataModel, id: InstanceId, destroyed: bool) {
        for node in std::iter::once(id).chain(dm.descendants(id)) {
            let Some(net) = self.ids.net(node) else { continue };
            if self.sent.remove(&net).is_some() {
                self.parent_on_players.remove(&net);
                if net.is_scene() && !self.absent_from_world.contains(&net) {
                    self.scene_removed.insert(net);
                }
            }
            if destroyed {
                self.ids.unbind_local(node);
            }
        }
    }

    /// Everything a player joining now needs on top of the world it loaded:
    /// what was removed, what was made, what moved, and what was written,
    /// with current values. `player` is its `Player` instance, for the
    /// containers only it sees.
    pub fn snapshot(&self, dm: &DataModel, player: Option<InstanceId>) -> WorldFrame {
        let sees = |a: &Audience| match a {
            Audience::Everyone => true,
            Audience::Owner(p) => Some(*p) == player,
        };
        let mut ops: Vec<ReplOp> = Vec::new();
        let loaded = |net: &NetId| self.in_world(*net) && self.sent.get(net).is_some_and(|a| sees(a));
        // The joiner has it from the world, or it is `game` or a service.
        let exists_there = |net: NetId| net == NetId::ROOT || loaded(&net);

        // 1. Loaded instances that moved go to their new parent now, or out of
        //    the tree for the moment when that parent is sent below, so no
        //    removal of their old parent takes them along.
        let mut later_moves: Vec<(NetId, NetId)> = Vec::new();
        for net in self.scene_moved.iter().filter(|n| loaded(*n)) {
            let parent = self.parent_on_players.get(net).copied().unwrap_or(NetId::NONE);
            if parent.is_none() || exists_there(parent) {
                ops.push(ReplOp::Reparent { id: *net, parent });
            } else {
                ops.push(ReplOp::Reparent { id: *net, parent: NetId::NONE });
                later_moves.push((*net, parent));
            }
        }

        // 2. What the world has and the host no longer does.
        for net in &self.scene_removed {
            ops.push(ReplOp::Destroy { id: *net });
        }

        // 3. Made at run time, or back after leaving: spawned, parents first.
        let mut spawns: Vec<(usize, InstanceId, NetId)> = Vec::new();
        for (net, a) in &self.sent {
            if self.in_world(*net) || *net == NetId::ROOT || !sees(a) {
                continue;
            }
            let Some(id) = self.ids.local(*net).filter(|id| dm.exists(*id)) else { continue };
            let mut depth = 0usize;
            let mut cur = dm.parent(id);
            while let Some(p) = cur {
                depth += 1;
                cur = dm.parent(p);
            }
            spawns.push((depth, id, *net));
        }
        spawns.sort_by_key(|(depth, _, net)| (*depth, *net));
        for (_, id, net) in spawns {
            if let Some(op) = self.spawn_op(dm, id, net) {
                ops.push(ReplOp::Spawn(op));
            }
        }

        // 4. The moves whose parent exists now.
        for (net, parent) in later_moves {
            ops.push(ReplOp::Reparent { id: net, parent });
        }
        let mut writes: BTreeMap<NetId, Vec<(String, WireValue)>> = BTreeMap::new();
        for (net, names) in self.scene_props.iter().filter(|(n, _)| loaded(*n)) {
            let Some(id) = self.ids.local(*net) else { continue };
            let entry = writes.entry(*net).or_default();
            for name in names {
                entry.push((name.clone(), self.wire(&dm.get_prop(id, name).unwrap_or_default())));
            }
        }
        for net in self.moved_by_physics.iter().filter(|n| loaded(*n)) {
            let Some(id) = self.ids.local(*net) else { continue };
            let entry = writes.entry(*net).or_default();
            for name in ["CFrame", "AssemblyLinearVelocity", "AssemblyAngularVelocity"] {
                if entry.iter().any(|(n, _)| n == name) {
                    continue;
                }
                if let Some(v) = dm.get_prop(id, name) {
                    entry.push((name.to_string(), self.wire(&v)));
                }
            }
        }
        for (net, props) in writes {
            if !props.is_empty() {
                ops.push(ReplOp::SetProps { id: net, props });
            }
        }
        for (net, names) in self.scene_attrs.iter().filter(|(n, _)| loaded(*n)) {
            let Some(id) = self.ids.local(*net) else { continue };
            let attributes = names
                .iter()
                .map(|n| (n.clone(), dm.get_attribute(id, n).map(|v| self.wire(&v)).unwrap_or(WireValue::Nil)))
                .collect();
            ops.push(ReplOp::SetAttributes { id: *net, attributes });
        }
        for net in self.scene_tags.iter().filter(|n| loaded(*n)) {
            let Some(id) = self.ids.local(*net) else { continue };
            ops.push(ReplOp::SetTags { id: *net, tags: dm.tags_of(id) });
        }
        WorldFrame { tick: self.tick, ops }
    }

    /// The ops of `out` a player may receive, as frames no larger than
    /// `max_weight` (see [`op_weight`]).
    pub fn frames_for(&self, out: &Outgoing, player: Option<InstanceId>, max_weight: usize) -> Vec<WorldFrame> {
        let ops: Vec<ReplOp> = out
            .iter()
            .filter(|(a, _)| match a {
                Audience::Everyone => true,
                Audience::Owner(p) => Some(*p) == player,
            })
            .map(|(_, op)| op.clone())
            .collect();
        split_frame(WorldFrame { tick: self.tick, ops }, max_weight)
    }
}

/// Roughly how many bytes an op encodes to: enough to keep frames under the
/// wire's limit without encoding twice.
pub fn op_weight(op: &ReplOp) -> usize {
    fn value(v: &WireValue) -> usize {
        match v {
            WireValue::String(s) => 16 + s.len(),
            WireValue::Enum(a, b) => 24 + a.len() + b.len(),
            WireValue::CFrame(..) => 100,
            WireValue::NumberSequence(k) => 16 + k.len() * 16,
            WireValue::ColorSequence(k) => 16 + k.len() * 32,
            WireValue::Array(items) => 16 + items.iter().map(value).sum::<usize>(),
            WireValue::Map(entries) => 16 + entries.iter().map(|(k, v)| 16 + k.len() + value(v)).sum::<usize>(),
            _ => 40,
        }
    }
    let pairs = |p: &[(String, WireValue)]| p.iter().map(|(k, v)| 16 + k.len() + value(v)).sum::<usize>();
    32 + match op {
        ReplOp::Spawn(s) => {
            s.class.len() + s.name.len() + pairs(&s.props) + pairs(&s.attributes) + s.tags.iter().map(|t| 16 + t.len()).sum::<usize>()
        }
        ReplOp::SetProps { props, .. } => pairs(props),
        ReplOp::SetAttributes { attributes, .. } => pairs(attributes),
        ReplOp::SetTags { tags, .. } => tags.iter().map(|t| 16 + t.len()).sum(),
        ReplOp::Remote { args, .. } => args.iter().map(value).sum(),
        ReplOp::Track { op: super::tracks::TrackWire::Load { content, name, .. }, .. } => 48 + content.len() + name.len(),
        ReplOp::Track { .. } => 72,
        _ => 16,
    }
}

/// Split a frame so no part outweighs `max_weight`. Order is kept; a single
/// op heavier than the limit travels alone.
pub fn split_frame(frame: WorldFrame, max_weight: usize) -> Vec<WorldFrame> {
    let mut frames = Vec::new();
    let mut current = WorldFrame { tick: frame.tick, ops: Vec::new() };
    let mut weight = 0usize;
    for op in frame.ops {
        let w = op_weight(&op);
        if !current.ops.is_empty() && weight + w > max_weight {
            frames.push(std::mem::replace(&mut current, WorldFrame { tick: frame.tick, ops: Vec::new() }));
            weight = 0;
        }
        weight += w;
        current.ops.push(op);
    }
    if !current.ops.is_empty() {
        frames.push(current);
    }
    frames
}
