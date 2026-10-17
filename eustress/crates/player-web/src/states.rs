//! # What the page is told
//!
//! The browser Player tells `/player/host.js` where it is through `onState`,
//! by a state name from a fixed set, and the page shows its own fixed text
//! for each. A host's or the gallery's own words never reach the page: any
//! host can set them, and a link's secrets must never be shown. So a refusal
//! is matched here against the words Eustress itself sends, each beside its
//! source, and anything else is the general `refused`.

use eustress_networking::session::NetNotice;

/// The Player is joining: the link was read and the session is opening.
pub const JOINING: &str = "joining";
/// The world is open and drawn.
pub const IN_WORLD: &str = "in-world";
/// The join failed before the world opened, for a reason that is not a refusal.
pub const JOIN_FAILED: &str = "join-failed";
/// The session ended after the world opened.
pub const DISCONNECTED: &str = "disconnected";
/// The host or the gallery turned the join away, for a reason not listed below.
pub const REFUSED: &str = "refused";

/// What a refusal says, as the session reports it (`player_message` in
/// eustress-networking session.rs: `the host refused the join: <reason>`).
const REFUSED_PREFIX: &str = "the host refused the join:";
/// A player the host removed (eustress-networking session.rs `kick`).
const REMOVED_PREFIX: &str = "the host removed this player:";

/// The refusals the page names, by the words their source sends: `exact`
/// for fixed text, otherwise a prefix the source always starts with.
const REFUSALS: &[(&str, Match)] = &[
    // eustress-networking session.rs `HostSession::full_reason`:
    // "This session is full (N of M players)."
    ("refused:full", Match::Prefix("This session is full (")),
    // eustress-networking session.rs `host_message` on a Hello of another
    // protocol: "This host speaks protocol N and your Player speaks M. ..."
    ("refused:protocol", Match::Prefix("This host speaks protocol ")),
    // The gallery's admit (infrastructure/cloudflare/api/src/live.mjs, code `used`).
    (
        "refused:ticket-used",
        Match::Exact("This ticket was already used. Join again from the gallery for a new one."),
    ),
    // The gallery's admit (live.mjs, code `not_available`).
    (
        "refused:not-available",
        Match::Exact("This session is not available right now."),
    ),
    // The Worker's ADULT_CONTENT_OFF (infrastructure/cloudflare/api/src/adult_content.mjs).
    (
        "refused:adult-content-off",
        Match::Exact("This listing is adult content. Turn on Adult content in your Profile to see it."),
    ),
    // The Worker's AGE_21_REQUIRED (adult_content.mjs).
    (
        "refused:age-21",
        Match::Exact("This listing is for adults 21 and over. It opens once your verified age is 21."),
    ),
];

#[derive(Debug, Clone, Copy)]
enum Match {
    Exact(&'static str),
    Prefix(&'static str),
}

impl Match {
    fn matches(self, said: &str) -> bool {
        match self {
            Match::Exact(text) => said == text,
            Match::Prefix(text) => said.starts_with(text),
        }
    }
}

/// The state for what a refusing host or gallery said.
pub fn refusal_state(said: &str) -> &'static str {
    let said = said.trim();
    REFUSALS
        .iter()
        .find(|(_, m)| m.matches(said))
        .map_or(REFUSED, |(state, _)| state)
}

/// The state a session notice puts the page in, if any. `in_world`: whether
/// the world had opened, after which every end is a lost connection.
pub fn state_for(notice: &NetNotice, in_world: bool) -> Option<&'static str> {
    let reason = match notice {
        NetNotice::JoinFailed { reason } | NetNotice::Disconnected { reason } => reason.trim(),
        _ => return None,
    };
    if in_world || matches!(notice, NetNotice::Disconnected { .. }) {
        return Some(DISCONNECTED);
    }
    if let Some(said) = reason.strip_prefix(REFUSED_PREFIX) {
        return Some(refusal_state(said));
    }
    if reason.starts_with(REMOVED_PREFIX) {
        return Some(REFUSED);
    }
    Some(JOIN_FAILED)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(reason: &str) -> NetNotice {
        NetNotice::JoinFailed {
            reason: reason.to_string(),
        }
    }

    /// Each refusal Eustress sends has its own state; any other words, a
    /// host's own included, are the general one.
    #[test]
    fn a_refusal_is_named_by_eustress_words_only() {
        for (said, want) in [
            ("This session is full (4 of 4 players).", "refused:full"),
            (
                "This host speaks protocol 9 and your Player speaks 8. Update whichever is older.",
                "refused:protocol",
            ),
            (
                "This ticket was already used. Join again from the gallery for a new one.",
                "refused:ticket-used",
            ),
            ("This session is not available right now.", "refused:not-available"),
            (
                "This listing is adult content. Turn on Adult content in your Profile to see it.",
                "refused:adult-content-off",
            ),
            (
                "This listing is for adults 21 and over. It opens once your verified age is 21.",
                "refused:age-21",
            ),
            (
                "The gallery could not confirm this join right now. Join again from the gallery.",
                "refused",
            ),
            ("BUY MY COIN at example.com", "refused"),
            ("", "refused"),
        ] {
            let reason = format!("the host refused the join: {said}");
            assert_eq!(state_for(&failed(&reason), false), Some(want), "{said}");
        }
    }

    /// A host's words that only contain Eustress's are not taken for them.
    #[test]
    fn a_refusal_matches_from_its_start() {
        assert_eq!(
            refusal_state("Not really. This session is full (1 of 1 players)."),
            "refused"
        );
        assert_eq!(
            refusal_state("This session is not available right now. Or ever."),
            "refused"
        );
    }

    /// A join that fails for any other reason is a failed join; once the
    /// world is open, every end is a lost connection.
    #[test]
    fn a_failure_is_a_failed_join_until_the_world_opens() {
        assert_eq!(
            state_for(&failed("could not reach the host (connect-failed)"), false),
            Some("join-failed")
        );
        assert_eq!(
            state_for(&failed("the host removed this player: spam"), false),
            Some("refused")
        );
        assert_eq!(
            state_for(
                &failed("the host refused the join: This session is full (2 of 2 players)."),
                true
            ),
            Some("disconnected")
        );
        let ended = NetNotice::Disconnected {
            reason: "the host closed the connection: closed".into(),
        };
        assert_eq!(state_for(&ended, false), Some("disconnected"));
        assert_eq!(state_for(&ended, true), Some("disconnected"));
        assert_eq!(
            state_for(&NetNotice::Downloading { received: 1, total: 2 }, false),
            None
        );
    }
}
