//! # Joined players in the Play tree
//!
//! A multiplayer host runs the simulation's scripts, so every player who joins
//! needs a `Player` in the host's Play tree: `Players:GetPlayers()`,
//! `PlayerAdded`, `PlayerRemoving`, and a `UserId` that `MarketplaceService`
//! can sell to. The networking session reports who joined and who left
//! (`NetNotice`); this keeps one `Player` per joined player while Play runs.
//!
//! A player that sent an identity ticket is checked with the Worker first
//! (`/api/identity/verify`, audience: this host's certificate pin), so its
//! `UserId` belongs to its account and is the same number every session, and
//! commerce knows which purchases are its own. A player with no ticket, or one
//! that fails the check, joins as a guest: it plays, and cannot buy.
//!
//! The host's own player keeps `UserId` 1 (seed.rs).
//!
//! Each joined player also gets a `Character`, built like the host's own
//! (`pull::build_character`) on the avatar the session keeps for that player
//! on this machine, so `CharacterAdded`, `Touched` and
//! `Players:GetPlayerFromCharacter` work the same for everyone.
//!
//! And a `PlayerGui`, filled from StarterGui as Roblox fills it: everything
//! when they join, then the `ResetOnSpawn` GUIs again each time their
//! character respawns. It reaches only that player, whose machine draws it
//! and runs its LocalScripts. This host does neither: it launches
//! LocalScripts only from its own player, and keeps joined players' GUIs off
//! its screen ([`hide_joined_player_guis`]).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use bevy::prelude::*;
use parking_lot::Mutex;
use serde_json::{json, Value};

use eustress_common::avatar::abilities::AvatarAbilities;
use eustress_common::avatar::climb::AvatarClimb;
use eustress_common::avatar::spawn::{AvatarBody, AvatarIntent, AvatarLocomotion};
use eustress_common::classes::BillboardGui;
use eustress_common::datamodel::{user_key, DataModel, DmEvent, DmValue, InstanceId};
use eustress_common::gui::billboard_renderer::GuiElementDisplay;
use eustress_networking::join_link::hex32;
use eustress_networking::wire::PeerId;
use eustress_networking::{NetNotice, NetReplica};

use super::PlayDataModel;

/// Guests' `UserId`s: 2^52 plus the peer id, above every account's.
const GUEST_USER_ID_BASE: u64 = 1 << 52;

const VERIFY_TIMEOUT: Duration = Duration::from_secs(10);

/// Everyone who joined this host, and their `Player` in the Play tree.
#[derive(Resource, Default)]
pub struct RemotePlayers {
    /// The pin this host serves under (`NetNotice::Hosting`): the audience
    /// every joining player's ticket has to name.
    pin: Option<[u8; 32]>,
    peers: BTreeMap<PeerId, RemotePlayer>,
    /// Identity checks that finished: the peer, and its account and name.
    inbox: Arc<Mutex<Vec<(PeerId, Option<(String, String)>)>>>,
    /// Players who left: `PlayerRemoving` fires first, the instance goes the
    /// frame after, so handlers still see it.
    leaving: Vec<InstanceId>,
    leaving_now: Vec<InstanceId>,
}

/// One joined player.
#[derive(Debug, Clone)]
pub struct RemotePlayer {
    pub peer: PeerId,
    /// The name it joined with, or its username once verified.
    pub name: String,
    /// The account its identity ticket proved.
    pub account: Option<String>,
    pub user_id: f64,
    /// Identity is settled: verified, failed, or never offered.
    settled: bool,
    verifying: bool,
    /// Its `Player` in the running Play tree.
    instance: Option<InstanceId>,
}

impl RemotePlayers {
    /// The joined player with this `UserId`.
    pub fn by_user_id(&self, user_id: f64) -> Option<&RemotePlayer> {
        self.peers.values().find(|p| p.settled && p.user_id == user_id)
    }

    pub fn by_peer(&self, peer: PeerId) -> Option<&RemotePlayer> {
        self.peers.get(&peer)
    }

    /// Everyone joined, in peer order.
    pub fn players(&self) -> impl Iterator<Item = &RemotePlayer> {
        self.peers.values()
    }

    /// The pin this host serves under, once it is listening.
    pub fn pin(&self) -> Option<[u8; 32]> {
        self.pin
    }
}

/// An account's `UserId`: a hash of its id, so the same account is the same
/// number every session (DataStore keys built from it keep working), within
/// 2..2^52 so it never meets the host's 1 or a guest's.
pub fn account_user_id(account: &str) -> f64 {
    // FNV-1a, 64-bit.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in account.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (2 + hash % (GUEST_USER_ID_BASE - 2)) as f64
}

fn guest_user_id(peer: PeerId) -> f64 {
    (GUEST_USER_ID_BASE + peer as u64) as f64
}

impl RemotePlayer {
    /// Its identity is settled: verified, failed, or never offered. Until then
    /// its account and `UserId` may still change.
    pub fn settled(&self) -> bool {
        self.settled
    }

    /// Its `Player` in the running Play tree, once made.
    pub fn instance(&self) -> Option<InstanceId> {
        self.instance
    }
}

/// Follow joins, leaves and identity tickets. Runs every frame, Play or not,
/// so no notice is missed.
pub fn track_remote_players(mut notices: MessageReader<NetNotice>, mut remote: ResMut<RemotePlayers>) {
    let remote = &mut *remote;
    for notice in notices.read() {
        match notice {
            NetNotice::Hosting { pin, .. } => remote.pin = Some(*pin),
            NetNotice::HostEnded { .. } => {
                for player in std::mem::take(&mut remote.peers).into_values() {
                    remote.leaving.extend(player.instance);
                }
                remote.pin = None;
            }
            NetNotice::PeerJoined { peer, name } => {
                remote.peers.insert(
                    *peer,
                    RemotePlayer {
                        peer: *peer,
                        name: name.clone(),
                        account: None,
                        user_id: guest_user_id(*peer),
                        settled: false,
                        verifying: false,
                        instance: None,
                    },
                );
            }
            NetNotice::PeerIdentity { peer, ticket } => {
                let (Some(pin), Some(player)) = (remote.pin, remote.peers.get_mut(peer)) else { continue };
                player.verifying = true;
                let inbox = remote.inbox.clone();
                let (peer, ticket, audience) = (*peer, ticket.clone(), hex32(&pin));
                let started = std::thread::Builder::new().name("eustress-identity".into()).spawn(move || {
                    let who = verify_ticket(&ticket, &audience);
                    inbox.lock().push((peer, who));
                });
                if started.is_err() {
                    player.verifying = false;
                }
            }
            NetNotice::PeerLeft { peer, .. } => {
                if let Some(player) = remote.peers.remove(peer) {
                    remote.leaving.extend(player.instance);
                }
            }
            _ => {}
        }
    }
    // A join with no ticket (it arrives in the same frame as the join) plays
    // as a guest at once.
    for player in remote.peers.values_mut() {
        if !player.settled && !player.verifying {
            player.settled = true;
        }
    }
    for (peer, who) in std::mem::take(&mut *remote.inbox.lock()) {
        let Some(player) = remote.peers.get_mut(&peer) else { continue };
        match who {
            Some((account, username)) => {
                player.user_id = account_user_id(&account);
                if !username.is_empty() {
                    player.name = username;
                }
                player.account = Some(account);
            }
            None => warn!("multiplayer: {} sent an identity ticket that did not verify; they play as a guest", player.name),
        }
        player.verifying = false;
        player.settled = true;
    }
}

/// The account and username a ticket proves, or None.
fn verify_ticket(ticket: &str, audience: &str) -> Option<(String, String)> {
    let url = format!("{}/api/identity/verify", super::commerce::api_base());
    let reply: Value = ureq::post(&url)
        .timeout(VERIFY_TIMEOUT)
        .send_json(json!({ "ticket": ticket, "audience": audience }))
        .ok()?
        .into_json()
        .ok()?;
    let account = reply.get("account_id")?.as_str()?.to_string();
    let username = reply.get("username").and_then(Value::as_str).unwrap_or_default().to_string();
    Some((account, username))
}

/// Keep one `Player` per joined player in the Play tree. Runs before the
/// scripts each Play frame, so a player who joined is in `GetPlayers()` and
/// `PlayerAdded` fires before scripts next run.
pub fn sync_remote_players(dm: Option<Res<PlayDataModel>>, mut remote: ResMut<RemotePlayers>) {
    let Some(dm) = dm else { return };
    let remote = &mut *remote;
    let mut g = dm.dm.lock();
    for gone in std::mem::take(&mut remote.leaving_now) {
        g.destroy(gone);
    }
    for gone in std::mem::take(&mut remote.leaving) {
        if g.exists(gone) {
            let key = g.get_prop(gone, "UserId").and_then(|v| v.as_number()).and_then(user_key);
            if let Some(key) = key {
                g.commerce.peer_passes_pending.remove(&key);
            }
            g.push_event(DmEvent::PlayerRemoving { player: gone });
            remote.leaving_now.push(gone);
        }
    }
    let Some(players) = g.get_service("Players") else { return };
    for player in remote.peers.values_mut() {
        if !player.settled || player.instance.is_some_and(|id| g.exists(id)) {
            continue;
        }
        let id = g.create_virtual("Player", &player.name, Some(players));
        let _ = g.set_prop(id, "DisplayName", DmValue::String(player.name.clone()));
        let _ = g.set_prop(id, "UserId", DmValue::Number(player.user_id));
        g.create_virtual("Backpack", "Backpack", Some(id));
        g.create_virtual("PlayerScripts", "PlayerScripts", Some(id));
        let gui = g.create_virtual("PlayerGui", "PlayerGui", Some(id));
        fill_player_gui(&mut g, gui, true);
        // Commerce reads a signed-in player's passes once it can; until then
        // `UserOwnsGamePassAsync` waits for them. Marked here, with
        // `PlayerAdded`, so no handler can ask before the mark is set.
        if player.account.is_some() {
            if let Some(key) = user_key(player.user_id) {
                g.commerce.peer_passes_pending.insert(key);
            }
        }
        g.push_event(DmEvent::PlayerAdded { player: id });
        player.instance = Some(id);
    }
}

/// Each joined player's `Character`, on the avatar the session keeps for them
/// here (`NetReplica`, moved by that player's own samples). Built once their
/// `Player` is in the tree, kept on the avatar every frame, and retired when
/// the avatar or the player goes; a new avatar (a changed appearance) gets a
/// new character, as a respawn does, and a respawn resets the player's
/// `ResetOnSpawn` GUIs.
#[allow(clippy::type_complexity)]
pub fn pull_remote_characters(
    dm: Option<Res<PlayDataModel>>,
    remote: Res<RemotePlayers>,
    avatars: Query<(
        Entity,
        &NetReplica,
        &Transform,
        &AvatarBody,
        &AvatarIntent,
        Option<&AvatarAbilities>,
        Option<&AvatarLocomotion>,
        Option<&AvatarClimb>,
    )>,
    mut bound: Local<HashMap<PeerId, (Entity, InstanceId, InstanceId)>>,
    mut spawned_before: Local<HashSet<InstanceId>>,
    mut session: Local<usize>,
) {
    let Some(dm) = dm else {
        bound.clear();
        spawned_before.clear();
        return;
    };
    // As in `pull_character`: a new Play session is a new tree.
    let tree = Arc::as_ptr(&dm.dm) as usize;
    if *session != tree {
        *session = tree;
        bound.clear();
        spawned_before.clear();
    }
    let mut g = dm.dm.lock();
    bound.retain(|peer, (avatar, player, model)| {
        let current = avatars.get(*avatar).is_ok_and(|(_, r, ..)| r.peer == *peer)
            && remote.by_peer(*peer).and_then(|p| p.instance()) == Some(*player)
            && g.exists(*player);
        if !current && g.exists(*model) {
            super::pull::retire_character(&mut g, *player, *model);
        }
        current
    });
    for (avatar, replica, tf, body, intent, abilities, loco, climb) in &avatars {
        let Some(player) = remote.by_peer(replica.peer).and_then(|p| p.instance()).filter(|p| g.in_tree(*p)) else {
            continue;
        };
        let model = match bound.get(&replica.peer) {
            Some((_, _, model)) => *model,
            None => {
                let abilities = abilities.copied().unwrap_or_default();
                let Some(model) = super::pull::build_character(&mut g, player, avatar, tf, body, abilities) else {
                    continue;
                };
                bound.insert(replica.peer, (avatar, player, model));
                // Joining filled the GUI; each respawn after the first
                // resets it.
                if !spawned_before.insert(player) {
                    if let Some(gui) = g.find_first_child_of_class(player, "PlayerGui", false) {
                        fill_player_gui(&mut g, gui, false);
                    }
                }
                model
            }
        };
        // The replica's locomotion runs here from its owner's intent and is
        // corrected onto the owner's samples, so its state is close to theirs.
        let movement = loco.map(|l| eustress_common::animation::character::locomotion_sample(l, climb));
        super::pull::place_character(&mut g, model, tf, body, intent, movement);
    }
}

/// Copy StarterGui into a joined player's `PlayerGui`. On joining (`first`)
/// every GUI is copied; on a respawn the copies with `ResetOnSpawn` (the
/// default) are replaced and the rest are kept, as in Roblox.
fn fill_player_gui(g: &mut DataModel, player_gui: InstanceId, first: bool) {
    let Some(starter) = g.find_service("StarterGui") else { return };
    let resets = |g: &DataModel, id: InstanceId| g.get_prop(id, "ResetOnSpawn").and_then(|v| v.as_bool()).unwrap_or(true);
    if !first {
        for old in g.children(player_gui).to_vec() {
            if resets(g, old) {
                g.destroy(old);
            }
        }
    }
    for template in g.children(starter).to_vec() {
        if !first && !resets(g, template) {
            continue;
        }
        if let Some(copy) = g.clone_instance(template) {
            let _ = g.set_parent(copy, Some(player_gui));
        }
    }
}

/// Joined players' GUIs are theirs alone. Play's draw step shows whatever is
/// under Players and re-derives visibility from `Enabled` and `Visible`
/// writes, so after it runs each frame, everything in a joined player's
/// `PlayerGui` is kept off this machine's screen, billboards included.
pub fn hide_joined_player_guis(
    dm: Option<Res<PlayDataModel>>,
    remote: Res<RemotePlayers>,
    mut displays: Query<&mut GuiElementDisplay>,
    mut billboards: Query<&mut BillboardGui>,
) {
    let Some(dm) = dm else { return };
    let g = dm.dm.lock();
    for player in remote.players().filter_map(|p| p.instance()) {
        let Some(gui) = g.find_first_child_of_class(player, "PlayerGui", false) else { continue };
        for id in g.descendants(gui) {
            let Some(entity) = g.entity_of(id).map(Entity::from_bits) else { continue };
            if let Ok(mut d) = displays.get_mut(entity) {
                if d.visible {
                    d.visible = false;
                }
            }
            if let Ok(mut b) = billboards.get_mut(entity) {
                if b.enabled {
                    b.enabled = false;
                }
            }
        }
    }
}

/// OnEnter(Editing): the Play tree is gone, and with it every `Player`. The
/// next Play session makes them again.
pub fn forget_remote_instances(mut remote: ResMut<RemotePlayers>) {
    for player in remote.peers.values_mut() {
        player.instance = None;
    }
    remote.leaving.clear();
    remote.leaving_now.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_ids_are_stable_per_account_and_never_meet_the_host_or_a_guest() {
        let a = account_user_id("0b6c3f0e-1111-4111-8111-aaaaaaaaaaaa");
        assert_eq!(a, account_user_id("0b6c3f0e-1111-4111-8111-aaaaaaaaaaaa"));
        assert_ne!(a, account_user_id("0b6c3f0e-1111-4111-8111-aaaaaaaaaaab"));
        for account in ["", "x", "player-1", "0b6c3f0e-1111-4111-8111-aaaaaaaaaaaa"] {
            let id = account_user_id(account);
            assert!(id >= 2.0 && id < GUEST_USER_ID_BASE as f64, "{account}: {id}");
            assert_eq!(id.fract(), 0.0);
        }
        assert!(guest_user_id(1) >= GUEST_USER_ID_BASE as f64);
        assert_eq!(eustress_common::datamodel::user_key(guest_user_id(7)), Some(GUEST_USER_ID_BASE + 7));
    }
}
