//! The Player's replica of the host's world: [`NetReplicaPlugin`], with
//! [`SceneKeys`] (and its `PlayerReplica` state). It lives in eustress-networking
//! (`player_replica`), so the desktop and browser Players register the same
//! one; these paths stay for the desktop Player's own modules.

pub use eustress_networking::player_replica::{NetReplicaPlugin, SceneKeys};
