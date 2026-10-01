// =============================================================================
// Eustress Web - Live Hosts API
// =============================================================================
// Whether someone is hosting a published simulation right now, and the join
// link their host produced. Studio sends a heartbeat every 30 s while it
// hosts, and the Worker reads the listing as Offline 90 s after the last one
// (infrastructure/cloudflare/api/src/live.mjs).
//
// Table of Contents:
// 1. Types
// 2. Live Hosts API Functions
// =============================================================================

use serde::Deserialize;
use super::{ApiClient, ApiError};

// -----------------------------------------------------------------------------
// 1. Types
// -----------------------------------------------------------------------------

/// `GET /api/simulations/{id}/live`
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct LiveHost {
    pub live: bool,
    /// The host's own join link, sent only while it is live:
    /// `eustress-player://join/<host>:<port>?key=<32 hex>&pin=<64 hex>`.
    #[serde(default)]
    pub link: Option<String>,
    /// True when the simulation is live but the Worker withheld the link,
    /// because the visitor is signed out and holds no guest lease.
    #[serde(default)]
    pub link_hidden: bool,
    #[serde(default)]
    pub players: u32,
    #[serde(default)]
    pub max_players: u32,
    /// The host's network protocol version.
    #[serde(default)]
    pub protocol: u32,
    /// RFC 3339 time the host started answering at this link.
    #[serde(default)]
    pub since: Option<String>,
}

impl LiveHost {
    /// The link to hand the Player, when the host is live and its link is one
    /// the page will open.
    pub fn join_link(&self) -> Option<&str> {
        self.link.as_deref().filter(|link| self.live && is_join_link(link))
    }
}

/// True for a link the page may give to `window.location`: the Player's join
/// scheme, only the characters the join grammar uses (letters, digits and
/// `-.:/?&=[]`), and no longer than the Worker stores. The Worker already
/// refuses anything but the host's exact grammar. The page checks again
/// because a `javascript:` URL given to `window.location` would run as script
/// on eustress.dev.
pub fn is_join_link(link: &str) -> bool {
    const SCHEME: &str = "eustress-player://join/";
    link.len() > SCHEME.len()
        && link.len() <= 512
        && link.starts_with(SCHEME)
        && link.bytes().all(|b| b.is_ascii_alphanumeric() || b"-.:/?&=[]".contains(&b))
}

// -----------------------------------------------------------------------------
// 2. Live Hosts API Functions
// -----------------------------------------------------------------------------

/// Whether the simulation is hosted right now. The client sends the signed-in
/// token, so an author also sees their own host on a listing that is not
/// approved yet.
pub async fn get_live_host(client: &ApiClient, simulation_id: &str) -> Result<LiveHost, ApiError> {
    client.get(&format!("/api/simulations/{}/live", simulation_id)).await
}
