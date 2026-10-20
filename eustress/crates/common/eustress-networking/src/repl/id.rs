//! Replicated identity: [`NetId`].
//!
//! A host and its players each hold their own DataModel, whose
//! [`InstanceId`]s are slot numbers private to that process. A `NetId` names
//! the same instance everywhere.
//!
//! - **Scene instances**, which every side loaded from the same world, hash
//!   their record key: the Space-relative path of the file that defines them.
//!   Each side computes the id from its own load, so no table is sent.
//! - **Services** hash their class, and `game` is [`NetId::ROOT`].
//! - **Runtime instances** (`Instance.new`, `Clone`, players, characters) get
//!   an id the host mints, with [`NetId::RUNTIME_BIT`] set, so it can never
//!   equal a scene id.
//!
//! The hash is FNV-1a 64 finished with SplitMix64, written out here: `std`'s
//! hasher may change between Rust releases, and a host and a player built by
//! different compilers must still agree.

use std::collections::HashMap;

use eustress_common::datamodel::InstanceId;
use serde::{Deserialize, Serialize};

/// An instance's id on the wire, the same on the host and on every player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NetId(pub u64);

impl NetId {
    /// No instance: a nil reference, or the parent of a detached instance.
    pub const NONE: NetId = NetId(0);
    /// `game`.
    pub const ROOT: NetId = NetId(1);
    /// Set on every id the host mints at run time.
    pub const RUNTIME_BIT: u64 = 1 << 63;

    pub fn is_none(self) -> bool {
        self.0 == 0
    }

    /// Minted by the host for an instance made while the session runs.
    pub fn is_runtime(self) -> bool {
        self.0 & Self::RUNTIME_BIT != 0
    }

    /// Loaded by every side from the world (or a service, or `game`).
    pub fn is_scene(self) -> bool {
        !self.is_none() && !self.is_runtime()
    }

    /// The runtime id with sequence number `n` (from 1).
    pub fn runtime(n: u64) -> NetId {
        NetId(Self::RUNTIME_BIT | (n & !Self::RUNTIME_BIT))
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// SplitMix64's finalizer: spreads FNV's weak low bits across the word.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A hash in the scene range: top bit clear, and never 0 (none) or 1 (root).
fn scene_hash(domain: &[u8], text: &str) -> NetId {
    let h = mix(fnv1a(fnv1a(FNV_OFFSET, domain), text.as_bytes())) & !NetId::RUNTIME_BIT;
    NetId(if h <= NetId::ROOT.0 { h + 2 } else { h })
}

/// The form of a record key both sides hash: forward slashes, no leading
/// `./` or `/`, and a folder instance named by its folder, so
/// `Workspace/Car/_instance.toml` and `Workspace/Car` are one key.
pub fn normalize_key(record_key: &str) -> String {
    let mut k = record_key.replace('\\', "/");
    while let Some(rest) = k.strip_prefix("./") {
        k = rest.to_string();
    }
    let k = k.trim_start_matches('/');
    let k = k.strip_suffix("/_instance.toml").unwrap_or(k);
    k.trim_end_matches('/').to_string()
}

/// The id of a scene instance, from the Space-relative path of the file that
/// defines it.
pub fn scene_net_id(record_key: &str) -> NetId {
    scene_hash(b"scene\0", &normalize_key(record_key))
}

/// The id of a service, by class (`Workspace`, `ReplicatedStorage`, ...).
pub fn service_net_id(class: &str) -> NetId {
    scene_hash(b"service\0", class)
}

/// The key `workspace.Terrain` goes by. Every side's tree has that handle
/// without a record of its own (the terrain root draws the ground), so both
/// bind it under this key instead of the host sending it as something new.
pub const TERRAIN_KEY: &str = "Workspace/Terrain";

/// Both directions between one process's [`InstanceId`]s and [`NetId`]s.
#[derive(Debug, Default, Clone)]
pub struct NetIdMap {
    to_local: HashMap<NetId, InstanceId>,
    to_net: HashMap<InstanceId, NetId>,
}

impl NetIdMap {
    /// Pair `net` with `local`, replacing any pairing either had. Returns the
    /// instance `net` was bound to before, when it was another one.
    pub fn bind(&mut self, net: NetId, local: InstanceId) -> Option<InstanceId> {
        let previous = self.to_local.insert(net, local).filter(|p| *p != local);
        if let Some(p) = previous {
            self.to_net.remove(&p);
        }
        if let Some(old_net) = self.to_net.insert(local, net) {
            if old_net != net {
                self.to_local.remove(&old_net);
            }
        }
        previous
    }

    pub fn local(&self, net: NetId) -> Option<InstanceId> {
        self.to_local.get(&net).copied()
    }

    pub fn net(&self, local: InstanceId) -> Option<NetId> {
        self.to_net.get(&local).copied()
    }

    pub fn unbind_local(&mut self, local: InstanceId) -> Option<NetId> {
        let net = self.to_net.remove(&local)?;
        self.to_local.remove(&net);
        Some(net)
    }

    pub fn unbind_net(&mut self, net: NetId) -> Option<InstanceId> {
        let local = self.to_local.remove(&net)?;
        self.to_net.remove(&local);
        Some(local)
    }

    pub fn len(&self) -> usize {
        self.to_local.len()
    }

    pub fn is_empty(&self) -> bool {
        self.to_local.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_ids_are_stable_and_never_runtime() {
        // Every spelling of one key hashes alike.
        assert_eq!(scene_net_id("Workspace/Car/Body"), scene_net_id("Workspace\\Car\\Body/_instance.toml"));
        assert_eq!(scene_net_id("./Workspace/Car/Body"), scene_net_id("/Workspace/Car/Body/"));
        assert_ne!(scene_net_id("Workspace/Car/Body"), scene_net_id("Workspace/Car/Wheel"));
        assert_ne!(scene_net_id("Workspace"), service_net_id("Workspace"));
        for key in ["", "a", "Workspace/Part.part.toml", "Lighting/Sky/_instance.toml"] {
            let id = scene_net_id(key);
            assert!(id.is_scene(), "{key:?} hashed to {id:?}");
            assert!(id != NetId::ROOT && !id.is_none());
        }
        assert!(NetId::runtime(7).is_runtime());
        assert!(!NetId::runtime(7).is_scene());
    }

    /// If these change, a host and a player built from different revisions
    /// stop agreeing on every scene instance.
    #[test]
    fn the_hash_is_pinned() {
        // FNV-1a 64 of "" is its offset basis; the finalizer is SplitMix64.
        assert_eq!(fnv1a(FNV_OFFSET, b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(FNV_OFFSET, b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(mix(0), 0);
        assert_eq!(mix(1), 0x5692_161d_100b_05e5);
        // The whole scheme, end to end (computed independently).
        assert_eq!(scene_net_id("Workspace/Car/Body"), NetId(0x716b_568c_2755_1a60));
    }

    #[test]
    fn many_keys_do_not_collide() {
        let mut seen = std::collections::HashSet::new();
        for i in 0..50_000 {
            assert!(seen.insert(scene_net_id(&format!("Workspace/Model{}/Part{}", i / 50, i % 50))));
        }
    }

    #[test]
    fn map_rebinding_keeps_both_directions_consistent() {
        let a = InstanceId(10);
        let b = InstanceId(11);
        let mut m = NetIdMap::default();
        assert_eq!(m.bind(NetId(5), a), None);
        assert_eq!(m.bind(NetId(5), b), Some(a));
        assert_eq!(m.net(a), None);
        assert_eq!(m.local(NetId(5)), Some(b));
        m.bind(NetId(6), b);
        assert_eq!(m.local(NetId(5)), None, "b moved to 6, so 5 is free");
        assert_eq!(m.unbind_local(b), Some(NetId(6)));
        assert!(m.is_empty());
    }
}
