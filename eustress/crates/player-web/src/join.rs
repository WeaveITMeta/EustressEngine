//! # Reading the join link the page hands over
//!
//! `/player/host.js` passes the `eustress-player://join/...` link the Play
//! page made for this click, its `ticket` and `api` appended, exactly as the
//! desktop Player receives it, and the API the page belongs to. The steps
//! are the desktop Player's, in its order (client `main.rs`): take the
//! ticket and the API out of the link ([`split_link_extras`]), parse what is
//! left ([`parse_join_link`]), and refuse a link for another API
//! ([`LinkExtras::check_api`]). Then the browser's own rule: the host's
//! certificate must be pinned. A browser takes a self-signed certificate only
//! by its hash (`serverCertificateHashes`), so an unpinned link, which the
//! desktop opens on its own machine alone, never opens here.
//!
//! A failure is a [`JoinRefusal`], one of a few fixed codes, so nothing this
//! returns can carry any part of the link.

use eustress_networking::join_link::{parse_join_link, split_link_extras, JoinTicket, LinkExtras};

/// The Player's link scheme, read in any case as the desktop reads it.
const PLAYER_SCHEME: &str = "eustress-player://";
/// The form [`split_link_extras`] and [`parse_join_link`] read a Player link
/// in, as the desktop rewrites it before reading.
const STUDIO_SCHEME: &str = "eustress://";

/// Why a join link was not used. Each is a fixed code; none quotes the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinRefusal {
    /// Not an `eustress-player://` join link (a `play/<id>` link included).
    NotAJoinLink,
    /// The link, its ticket or its API is malformed, or it names either twice.
    Malformed,
    /// The link belongs to another API than this page's.
    OtherApi,
    /// The link does not pin the host's certificate.
    Unpinned,
}

impl JoinRefusal {
    /// The code the page is given.
    pub fn code(self) -> &'static str {
        match self {
            JoinRefusal::NotAJoinLink => "not-a-join-link",
            JoinRefusal::Malformed => "malformed-link",
            JoinRefusal::OtherApi => "other-api",
            JoinRefusal::Unpinned => "unpinned-link",
        }
    }
}

/// A join, read and checked: where the session opens, how the host is
/// checked, and the identity its `Hello` carries. Not `Clone`, and its
/// `Debug` prints none of the URL (the join key is in its path), the pin or
/// the ticket.
pub struct JoinPlan {
    /// `https://<host>:<port>/join/<key>`, the URL `net.js` opens.
    pub url: String,
    /// `<host>:<port>`, which says where without the key.
    pub remote: String,
    /// The SHA-256 of the host's certificate.
    pub pin: [u8; 32],
    /// The ticket the gallery's Play page put in the link, for the one
    /// `Hello` that sends it (`docs/networking/PLAYER_JOIN_TICKET.md`).
    pub identity: Option<String>,
}

impl JoinPlan {
    /// Whether the join carries the gallery's ticket, which works once: a
    /// failed join is joined again from the gallery.
    pub fn gallery_ticket(&self) -> bool {
        self.identity.is_some()
    }
}

impl std::fmt::Debug for JoinPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoinPlan")
            .field("remote", &self.remote)
            .field(
                "identity",
                &format_args!(
                    "{}",
                    if self.identity.is_some() {
                        "<set>"
                    } else {
                        "<none>"
                    }
                ),
            )
            .finish_non_exhaustive()
    }
}

/// Read `join` for a page of the API at `api_base`, as the desktop Player
/// reads its own link, and require the host's pin.
pub fn plan_join(join: &str, api_base: &str) -> Result<JoinPlan, JoinRefusal> {
    let join = join.trim();
    let rest = join
        .get(..PLAYER_SCHEME.len())
        .filter(|head| head.eq_ignore_ascii_case(PLAYER_SCHEME))
        .map(|_| &join[PLAYER_SCHEME.len()..])
        .ok_or(JoinRefusal::NotAJoinLink)?;
    let (link_text, extras): (String, LinkExtras) =
        split_link_extras(&format!("{STUDIO_SCHEME}{rest}")).map_err(|_| JoinRefusal::Malformed)?;
    if link_text.starts_with("eustress://play/") {
        return Err(JoinRefusal::NotAJoinLink);
    }
    let link = parse_join_link(&link_text).map_err(|_| JoinRefusal::Malformed)?;
    extras
        .check_api(api_base)
        .map_err(|_| JoinRefusal::OtherApi)?;
    let pin = link.pin.ok_or(JoinRefusal::Unpinned)?;
    Ok(JoinPlan {
        url: link.webtransport_url(),
        remote: format!("{}:{}", link.host, link.port),
        pin,
        identity: extras.ticket.map(JoinTicket::into_secret),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "Qx7key0123456789SECRET";
    const TICKET: &str = "tkt.SECRET-0123_abc";
    const STAGING: &str = "https://api-staging.eustress.dev";
    const PRODUCTION: &str = "https://api.eustress.dev";

    fn pin() -> String {
        "ab".repeat(32)
    }

    fn link(extra: &str) -> String {
        format!(
            "eustress-player://join/192.168.1.20:7777?key={KEY}&pin={}{extra}",
            pin()
        )
    }

    fn leaks(text: &str) -> bool {
        text.contains("SECRET") || text.contains(&pin()) || text.contains("eustress")
    }

    /// A gallery's link opens with the desktop's reading: the WebTransport
    /// URL with its key, the pin, and the ticket for the one Hello.
    #[test]
    fn a_pinned_gallery_link_is_planned_as_the_desktop_reads_it() {
        let plan = plan_join(&link(&format!("&ticket={TICKET}&api=staging")), STAGING)
            .expect("a good link");
        assert_eq!(plan.url, format!("https://192.168.1.20:7777/join/{KEY}"));
        assert_eq!(plan.remote, "192.168.1.20:7777");
        assert_eq!(plan.pin, [0xab; 32]);
        assert_eq!(plan.identity.as_deref(), Some(TICKET));
        assert!(plan.gallery_ticket());
        assert!(
            !leaks(&format!("{plan:?}")),
            "the plan's Debug carries no secret: {plan:?}"
        );
    }

    /// A link without a ticket or an API is a direct join, for this page's
    /// API, with no identity.
    #[test]
    fn a_link_without_extras_joins_with_no_identity() {
        let plan = plan_join(&format!("  {}  ", link("")), PRODUCTION).expect("a good link");
        assert_eq!(plan.identity, None);
        assert!(!plan.gallery_ticket());
        let shouty = link("").replacen("eustress-player://", "EUSTRESS-PLAYER://", 1);
        assert!(
            plan_join(&shouty, PRODUCTION).is_ok(),
            "the scheme reads in any case, as on the desktop"
        );
    }

    /// Every refusal is its own fixed code, and none quotes the link.
    #[test]
    fn a_link_is_refused_by_fixed_codes_only() {
        let unpinned =
            format!("eustress-player://join/192.168.1.20:7777?key={KEY}&ticket={TICKET}");
        let cases = [
            (
                format!("https://192.168.1.20:7777/join/{KEY}"),
                JoinRefusal::NotAJoinLink,
            ),
            (
                format!("eustress://join/192.168.1.20:7777?key={KEY}&pin={}", pin()),
                JoinRefusal::NotAJoinLink,
            ),
            (
                "eustress-player://play/07d084d8-9098?api=staging".to_string(),
                JoinRefusal::NotAJoinLink,
            ),
            (link("&ticket=bad ticket SECRET"), JoinRefusal::Malformed),
            (
                link(&format!("&ticket={TICKET}&ticket={TICKET}")),
                JoinRefusal::Malformed,
            ),
            (link("&api=SECRETserver"), JoinRefusal::Malformed),
            (
                format!("eustress-player://join/192.168.1.20:7777?key={KEY}&pin=12SECRET"),
                JoinRefusal::Malformed,
            ),
            (
                format!(
                    "eustress-player://join/192.168.1.20?key={KEY}&pin={}",
                    pin()
                ),
                JoinRefusal::Malformed,
            ),
            (link("&api=production"), JoinRefusal::OtherApi),
            (unpinned, JoinRefusal::Unpinned),
        ];
        for (text, want) in cases {
            let got = plan_join(&text, STAGING).expect_err("refused");
            assert_eq!(got, want, "{}", want.code());
            assert!(!leaks(&format!("{got:?}")) && !leaks(got.code()));
        }
    }
}
