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
    /// True when every reported server is full. The Worker then omits the join link.
    #[serde(default)]
    pub all_full: bool,
    /// One row per process. Empty when the Worker has not started sending them.
    #[serde(default)]
    pub servers: Vec<LiveServer>,
}

/// One process on a listing, as `GET /live` reports it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct LiveServer {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub players: u32,
    #[serde(default)]
    pub max_players: u32,
    #[serde(default)]
    pub free: u32,
    #[serde(default)]
    pub full: bool,
    /// The host's region name (`us-east-1`), not the visitor's country.
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    /// Round trip from that server to the API. Absent when it was not measured.
    #[serde(default)]
    pub latency_ms: Option<u32>,
    #[serde(default)]
    pub hardware: Option<LiveHardware>,
    #[serde(default)]
    pub link: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct LiveHardware {
    #[serde(default)]
    pub gpu: String,
    #[serde(default)]
    pub cpu: String,
    #[serde(default)]
    pub tier: String,
}

impl LiveServer {
    fn tier(&self) -> u8 {
        match self.hardware.as_ref().map(|h| h.tier.as_str()).unwrap_or("unknown") {
            "high" => 3,
            "mid" => 2,
            "low" => 1,
            _ => 0,
        }
    }
}

/// "N ms to the API", or "not measured". A missing value is not 0 ms.
pub fn latency_label(latency_ms: Option<u32>) -> String {
    match latency_ms {
        Some(ms) if ms >= 1 => format!("{ms} ms to the API"),
        _ => "not measured".to_string(),
    }
}

fn ping_score(latency_ms: Option<u32>) -> f64 {
    match latency_ms {
        Some(ms) if ms >= 1 => 1.0 - (ms.min(300) as f64) / 300.0,
        _ => 0.0,
    }
}

fn rank_score(server: &LiveServer) -> f64 {
    let max = server.max_players.max(1) as f64;
    let players = (server.players as f64).min(max);
    let fullness = (players / max).clamp(0.0, 1.0);
    0.5 * fullness + 0.3 * (server.tier() as f64 / 3.0) + 0.2 * ping_score(server.latency_ms.filter(|ms| *ms >= 1))
}

/// Busiest non-full first, then more players. Full servers last.
/// Same weights as the Worker: 0.5 fullness, 0.3 hardware, 0.2 ping.
pub fn rank_servers(mut servers: Vec<LiveServer>) -> Vec<LiveServer> {
    servers.sort_by(|a, b| {
        a.full.cmp(&b.full).then_with(|| rank_score(b).partial_cmp(&rank_score(a)).unwrap_or(std::cmp::Ordering::Equal)).then_with(|| b.players.cmp(&a.players))
    });
    servers
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

/// True for the final link the page gives to `window.location`: the host's join link or a solo link, with the `ticket`
/// and `api` parameters the Play chooser appended. Only the Player's two schemes, only the characters the grammar
/// uses (letters, digits and `-.:/?&=[]_`), and no longer than a Worker link (512), a ticket (2,048) and the
/// parameter names (32). `is_join_link` still checks the Worker's own link before anything is appended.
pub fn is_launch_link(link: &str) -> bool {
    const MAX: usize = 512 + 2_048 + 32;
    let rest = link
        .strip_prefix("eustress-player://join/")
        .or_else(|| link.strip_prefix("eustress-player://play/"));
    rest.is_some_and(|rest| !rest.is_empty())
        && link.len() <= MAX
        && link.bytes().all(|b| b.is_ascii_alphanumeric() || b"-.:/?&=[]_".contains(&b))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn join() -> String {
        format!("eustress-player://join/192.168.1.20:7777?key={}&pin={}", "0123456789abcdef".repeat(2), "ab".repeat(32))
    }

    #[test]
    fn the_final_link_may_carry_a_ticket_and_the_api_name_and_the_workers_own_link_stays_capped_at_512() {
        let ticketed = format!("{}&ticket=eyJhbGciOi_JIUzI1.NiIsInR5-cCI6&api=staging", join());
        assert!(is_launch_link(&ticketed));
        assert!(is_launch_link(&join()));
        assert!(is_launch_link("eustress-player://play/aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa?api=production"));
        assert!(is_launch_link("eustress-player://play/aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa"));
        assert!(is_launch_link("eustress-player://join/[::1]:7777?key=a&pin=b"));
        // The largest link the contract allows is still accepted ...
        let base = "eustress-player://join/h?pin=".to_string();
        let max = format!("{base}{}", "a".repeat(512 + 2_048 + 32 - base.len()));
        assert_eq!(max.len(), 512 + 2_048 + 32);
        assert!(is_launch_link(&max));
        assert!(!is_launch_link(&format!("{max}a")));
        // ... but the Worker's own link is not allowed to grow past 512 because of it.
        let worker_long = format!("{base}{}", "a".repeat(513 - base.len()));
        assert_eq!(worker_long.len(), 513);
        assert!(!is_join_link(&worker_long));
        assert!(is_launch_link(&worker_long));
        assert!(!is_join_link(&ticketed), "the Worker's own check is unchanged: no underscore, no ticket");
    }

    #[test]
    fn the_final_link_check_refuses_other_schemes_and_any_character_outside_the_grammar() {
        let ok = join();
        for bad in [
            "javascript:alert(1)".to_string(),
            "https://example.com/".to_string(),
            "http://eustress.dev/".to_string(),
            "eustress://join/1.2.3.4:5?key=a&pin=b".to_string(),
            "eustress-player://exec/calc".to_string(),
            "eustress-player://join/".to_string(),
            "eustress-player://play/".to_string(),
            "".to_string(),
            format!("{ok} x"),
            format!("{ok}&ticket=a b"),
            format!("{ok}\""),
            format!("{ok}'"),
            format!("{ok}%20"),
            format!("{ok}#frag"),
            format!("{ok}+"),
            format!("{ok}\n"),
            format!("{ok}<"),
            format!("{ok}\u{e9}"),
            format!(" {ok}"),
            format!("{ok}{}", "a".repeat(3_000)),
        ] {
            assert!(!is_launch_link(&bad), "{bad:.80}");
        }
    }
}
