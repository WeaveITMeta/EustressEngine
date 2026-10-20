//! What the world lane carries, and which instances it may carry.

use eustress_common::datamodel::{DataModel, InstanceId};
use serde::{Deserialize, Serialize};

use super::id::NetId;
use super::tracks::TrackWire;
use super::value::WireValue;
use crate::wire::PeerId;

/// A new instance on a player: made by a script, made by the engine (a
/// player, a character), or a scene instance coming back into view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpawnOp {
    pub id: NetId,
    pub class: String,
    pub name: String,
    /// [`NetId::NONE`] leaves it detached.
    pub parent: NetId,
    pub props: Vec<(String, WireValue)>,
    pub attributes: Vec<(String, WireValue)>,
    pub tags: Vec<String>,
    /// For a `Player`: the session peer it stands for, so a player binds the
    /// one naming itself to its own `LocalPlayer` instead of making another.
    pub peer: Option<PeerId>,
}

/// One change to the replicated tree. A frame's ops apply in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReplOp {
    Spawn(SpawnOp),
    /// Property writes; `Name` included.
    SetProps { id: NetId, props: Vec<(String, WireValue)> },
    /// Attribute writes; [`WireValue::Nil`] removes one.
    SetAttributes { id: NetId, attributes: Vec<(String, WireValue)> },
    /// The whole tag set.
    SetTags { id: NetId, tags: Vec<String> },
    /// [`NetId::NONE`] detaches.
    Reparent { id: NetId, parent: NetId },
    /// Gone, with everything under it.
    Destroy { id: NetId },
    /// A host script fired `remote` at this player (`FireClient`,
    /// `FireAllClients`). Ordered with the writes around it.
    Remote { remote: NetId, args: Vec<WireValue> },
    /// `Sound:Play()` and friends: 0 play, 1 stop, 2 pause, 3 resume.
    Sound { sound: NetId, action: u8 },
    /// `ParticleEmitter:Emit(count)`.
    Emit { emitter: NetId, count: u32 },
    /// An animation track change a machine's script made (`from`: the host's
    /// own scripts are peer 0). A player never applies its own back.
    Track { from: PeerId, op: TrackWire },
}

impl ReplOp {
    /// The instance the op is about; a track change names no instance.
    pub fn subject(&self) -> NetId {
        match self {
            ReplOp::Track { .. } => NetId::NONE,
            ReplOp::Spawn(s) => s.id,
            ReplOp::SetProps { id, .. }
            | ReplOp::SetAttributes { id, .. }
            | ReplOp::SetTags { id, .. }
            | ReplOp::Reparent { id, .. }
            | ReplOp::Destroy { id } => *id,
            ReplOp::Remote { remote, .. } => *remote,
            ReplOp::Sound { sound, .. } => *sound,
            ReplOp::Emit { emitter, .. } => *emitter,
        }
    }
}

/// Everything the host's tree did in one tick that a player can see.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorldFrame {
    pub tick: u64,
    pub ops: Vec<ReplOp>,
}

/// Containers whose contents players see. Everything else (ServerScriptService,
/// ServerStorage, services with no instances) stays on the host.
pub const REPLICATED_SERVICES: &[&str] = &[
    "Workspace",
    "ReplicatedStorage",
    "ReplicatedFirst",
    "Lighting",
    "Players",
    "Teams",
    "StarterGui",
    "StarterPack",
    "StarterPlayer",
    "SoundService",
    "Chat",
];

/// Children of a `Player` only that player sees.
pub const PRIVATE_PLAYER_CONTAINERS: &[&str] = &["PlayerGui", "PlayerScripts", "Backpack"];

/// Who may see an instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Audience {
    Everyone,
    /// Only the player whose `Player` instance this is.
    Owner(InstanceId),
}

/// Who may see `id` in `dm`, or `None` when it stays on the host: outside
/// the replicated services, detached, the host's own camera, or destroyed.
pub fn audience(dm: &DataModel, id: InstanceId) -> Option<Audience> {
    let root = dm.root();
    if id == root {
        return Some(Audience::Everyone);
    }
    let inst = dm.get(id)?;
    if inst.destroyed {
        return None;
    }
    // Walk up to the service, remembering the last two instances passed so
    // a private container under a Player is recognised on the way.
    let mut chain: Vec<InstanceId> = vec![id];
    let mut cur = inst.parent;
    loop {
        let p = cur?;
        if p == root {
            break;
        }
        chain.push(p);
        cur = dm.get(p)?.parent;
    }
    let service = *chain.last()?;
    let service_class = dm.class_of(service)?;
    if !REPLICATED_SERVICES.contains(&service_class) {
        return None;
    }
    if service_class == "Workspace" && chain.len() >= 2 {
        // The host's camera; each player has its own.
        let top = chain[chain.len() - 2];
        if dm.class_of(top) == Some("Camera") {
            return None;
        }
    }
    if service_class == "Players" && chain.len() >= 3 {
        // Players / <Player> / <container> / ...
        let player = chain[chain.len() - 2];
        let container = chain[chain.len() - 3];
        let private = dm.class_of(player) == Some("Player")
            && dm.class_of(container).is_some_and(|c| PRIVATE_PLAYER_CONTAINERS.contains(&c));
        if private {
            return Some(Audience::Owner(player));
        }
    }
    Some(Audience::Everyone)
}

/// Whether property `prop` of a `class` instance is sent at all.
pub fn replicates_prop(class: &str, prop: &str) -> bool {
    match prop {
        // Structure travels as ops of its own.
        "Parent" | "ClassName" => false,
        // The DataModel's markers for attribute and tag writes.
        p if p.starts_with("__") => false,
        // Server code never leaves the host.
        "Source" => class != "Script",
        // Each player has its own camera and is its own LocalPlayer, as in
        // Roblox: the host's values name the host's camera and player, so
        // sending them would only clear or retarget the player's own.
        "CurrentCamera" => class != "Workspace",
        "LocalPlayer" => class != "Players",
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> (DataModel, InstanceId, InstanceId) {
        let mut dm = DataModel::new();
        let root = dm.root();
        for class in ["Workspace", "Players", "ServerStorage", "ReplicatedStorage"] {
            dm.create_virtual(class, class, Some(root));
        }
        let players = dm.find_service("Players").unwrap();
        let alice = dm.create_virtual("Player", "Alice", Some(players));
        let gui = dm.create_virtual("PlayerGui", "PlayerGui", Some(alice));
        (dm, alice, gui)
    }

    #[test]
    fn server_containers_and_the_host_camera_stay_home() {
        let (mut dm, alice, gui) = tree();
        let ws = dm.find_service("Workspace").unwrap();
        let ss = dm.find_service("ServerStorage").unwrap();
        let part = dm.create("Part");
        dm.set_parent(part, Some(ws)).unwrap();
        let secret = dm.create("Part");
        dm.set_parent(secret, Some(ss)).unwrap();
        let cam = dm.create("Camera");
        dm.set_parent(cam, Some(ws)).unwrap();
        let loose = dm.create("Part");

        assert_eq!(audience(&dm, part), Some(Audience::Everyone));
        assert_eq!(audience(&dm, secret), None);
        assert_eq!(audience(&dm, cam), None);
        assert_eq!(audience(&dm, loose), None, "detached");
        assert_eq!(audience(&dm, alice), Some(Audience::Everyone));
        assert_eq!(audience(&dm, gui), Some(Audience::Owner(alice)));

        let label = dm.create("TextLabel");
        dm.set_parent(label, Some(gui)).unwrap();
        assert_eq!(audience(&dm, label), Some(Audience::Owner(alice)));
        let stats = dm.create("Folder");
        dm.set_parent(stats, Some(alice)).unwrap();
        assert_eq!(audience(&dm, stats), Some(Audience::Everyone), "leaderstats are public");

        dm.destroy(part);
        assert_eq!(audience(&dm, part), None);

        // Each player keeps its own CurrentCamera; a Camera's own writes stay
        // home with it, and any other object's CurrentCamera-named property
        // is ordinary.
        assert!(!replicates_prop("Workspace", "CurrentCamera"));
        assert!(replicates_prop("Model", "CurrentCamera"));
    }

    #[test]
    fn server_script_source_is_never_a_property_on_the_wire() {
        assert!(!replicates_prop("Script", "Source"));
        assert!(replicates_prop("LocalScript", "Source"));
        assert!(replicates_prop("ModuleScript", "Source"));
        assert!(!replicates_prop("Part", "__attributes"));
        assert!(!replicates_prop("Part", "Parent"));
        assert!(replicates_prop("Part", "Name"));
    }
}
