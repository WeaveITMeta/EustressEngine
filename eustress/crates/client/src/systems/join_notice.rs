//! # Why the Player stopped, in plain words
//!
//! The session and the download report a failure as one line written for the
//! log ("the host refused the join: ...", "could not reach the host at ...").
//! These turn that line into what the Player shows on screen
//! (`eustress_play_runtime::screen_notice`): a headline, what happened, and
//! what the person can do about it. The log keeps the original line.
//!
//! Plain `std` only, so the wording is tested without the rest of the Player.

/// A failure as the Player shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlainNotice {
    pub title: String,
    pub detail: String,
    pub hint: Option<String>,
}

fn notice(title: &str, detail: impl Into<String>, hint: Option<&str>) -> PlainNotice {
    PlainNotice { title: title.to_string(), detail: sentence(&detail.into()), hint: hint.map(str::to_string) }
}

/// A log line as a sentence: a capital first letter and a closing full stop.
fn sentence(text: &str) -> String {
    let text = text.trim();
    let mut chars = text.chars();
    let Some(first) = chars.next() else { return String::new() };
    let mut out: String = first.to_uppercase().chain(chars).collect();
    if !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

const REJOIN: &str = "Open the join link again to rejoin.";
const SIGN_IN: &str = "This Player is not signed in, and this host admits signed-in players. \
                       Sign in to Eustress, then open the join link again.";
/// For a join that sent a gallery's ticket, which lets a player in once.
const GALLERY_REJOIN: &str = "Join again from the gallery.";

/// Joining a host failed before the player was in the world. `signed_in`:
/// whether the Player sent an account with its join, which is what a host
/// that turns away guests needs.
pub fn join_failed(reason: &str, signed_in: bool) -> PlainNotice {
    let reason = reason.trim();
    if let Some(said) = reason.strip_prefix("the host refused the join:") {
        let said = said.trim();
        let hint = if !signed_in {
            Some(SIGN_IN)
        } else if said.contains("full") {
            Some("Try again when someone leaves.")
        } else if said.contains("protocol") || said.contains("Join again") {
            None // The host's own words say what to do.
        } else {
            Some("Ask the host, or open the join link again.")
        };
        let detail = if said.is_empty() { "The host gave no reason." } else { said };
        return notice("The host did not let you in", detail, hint);
    }
    if let Some(said) = reason.strip_prefix("the host removed this player:") {
        return notice("The host removed you", said, None);
    }
    if reason.contains("is from an earlier hosting session") {
        return notice(
            "This join link has expired",
            "The host has started a new hosting session since this link was made.",
            Some("Ask the host for the current link and open it."),
        );
    }
    if reason.contains("must carry the host's certificate pin") {
        return notice(
            "This join link is incomplete",
            "The link is missing the part that proves which host it names.",
            Some("Copy the whole link the host shows, including everything after the question mark."),
        );
    }
    if reason.contains("did not answer within") {
        return notice(
            "The host did not answer",
            reason,
            Some("Check that the host is still running and that this computer can reach it."),
        );
    }
    if reason.starts_with("could not reach the host") {
        return notice(
            "Could not reach the host",
            reason.split(". Check").next().unwrap_or(reason),
            Some("Check that the host is running, that its address is right, and that the link is current."),
        );
    }
    if let Some(said) = reason.strip_prefix("the host closed the connection:") {
        return notice("The host closed the connection", said, Some(REJOIN));
    }
    if reason.starts_with("could not open a network socket")
        || reason.starts_with("could not start the network")
        || reason.starts_with("could not open the session stream")
    {
        return notice(
            "This computer could not connect",
            reason,
            Some("A firewall or another program may be blocking the Player. Try again in a moment."),
        );
    }
    if reason.starts_with("the host sent an unreadable message") {
        return notice(
            "The host runs a different version",
            reason,
            Some("Update whichever of the Player and the host's Studio is older."),
        );
    }
    notice("Could not join the session", reason, Some(REJOIN))
}

/// The player was in the world, and the session ended.
pub fn disconnected(reason: &str) -> PlainNotice {
    let reason = reason.trim();
    if let Some(said) = reason.strip_prefix("the host removed this player:") {
        return notice("The host removed you", said, None);
    }
    if let Some(said) = reason.strip_prefix("the host closed the connection:") {
        return notice("The session ended", said, Some(REJOIN));
    }
    notice("Disconnected from the session", reason, Some(REJOIN))
}

/// [`join_failed`] for a join that sent the ticket a gallery's Play page put
/// in its link. That ticket works once and the Player never sends it again,
/// so the way back in is the gallery, which makes a new one: the same link
/// would be refused.
pub fn join_failed_with_gallery_ticket(reason: &str) -> PlainNotice {
    via_gallery(join_failed(reason, true), reason)
}

/// [`disconnected`] for a session joined with a gallery's ticket.
pub fn disconnected_with_gallery_ticket(reason: &str) -> PlainNotice {
    via_gallery(disconnected(reason), reason)
}

/// The gallery's hint in place of the usual one. A player the host removed
/// gets no hint, as everywhere, and words that already send the player to
/// the gallery are not repeated.
fn via_gallery(mut n: PlainNotice, reason: &str) -> PlainNotice {
    let removed = reason.trim().starts_with("the host removed this player:");
    let said = n.detail.to_ascii_lowercase().contains("join again from the gallery");
    if !removed && !said {
        n.hint = Some(GALLERY_REJOIN.to_string());
    }
    n
}

/// The world arrived and could not be opened.
pub fn world_failed(reason: &str) -> PlainNotice {
    notice(
        "The world could not be opened",
        reason,
        Some("Try again. If it keeps happening, tell whoever made this world."),
    )
}

/// A published simulation could not be downloaded. `error` is
/// `space_fetch`'s line: an HTTP status first, or the request that failed.
pub fn fetch_failed(error: &str, signed_in: bool) -> PlainNotice {
    let error = error.trim();
    let status = error.split_whitespace().next().and_then(|w| w.parse::<u16>().ok());
    match status {
        Some(401 | 403) => notice(
            "This simulation is not open to you",
            "Eustress did not let this Player download it.",
            Some(if signed_in {
                "It may be private. Ask its creator for access."
            } else {
                "This Player is not signed in. Sign in to Eustress, then press Play again."
            }),
        ),
        Some(404) => notice(
            "This simulation was not found",
            "It may have been unpublished or removed.",
            Some("Go back to the gallery and pick it again."),
        ),
        Some(code) if code >= 500 => notice(
            "Eustress is having trouble",
            format!("The download service answered with an error ({code})."),
            Some("Try again in a few minutes."),
        ),
        _ if error.starts_with("GET ") => notice(
            "Could not reach Eustress",
            "The download did not get an answer.",
            Some("Check this computer's internet connection, then try again."),
        ),
        _ => notice("The simulation could not be downloaded", error, Some("Try again. If it keeps happening, tell its creator.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The host's own words come through, with the help that fits them.
    #[test]
    fn a_refusal_shows_the_hosts_words() {
        let n = join_failed("the host refused the join: This session is full.", true);
        assert_eq!(n.title, "The host did not let you in");
        assert_eq!(n.detail, "This session is full.");
        assert_eq!(n.hint.as_deref(), Some("Try again when someone leaves."));

        let gallery = "the host refused the join: The gallery could not confirm this join right now. Join again from the gallery.";
        let n = join_failed(gallery, true);
        assert_eq!(n.detail, "The gallery could not confirm this join right now. Join again from the gallery.");
        assert_eq!(n.hint, None, "the gallery's words already say what to do");
    }

    /// A Player that joined as a guest is told to sign in, whatever the host said.
    #[test]
    fn a_refused_guest_is_told_to_sign_in() {
        let n = join_failed("the host refused the join: The gallery did not admit this join.", false);
        assert_eq!(n.detail, "The gallery did not admit this join.");
        assert!(n.hint.unwrap().contains("Sign in to Eustress"));
    }

    /// Every failure the join path writes gets its own headline, and a line
    /// nobody planned for still reads as a sentence.
    #[test]
    fn each_join_failure_has_a_headline() {
        let target = "192.168.1.20:7777";
        let cases = [
            (format!("this link is from an earlier hosting session: the host at {target} is not the one it names. Copy the current link from the host's Output."), "This join link has expired"),
            (format!("{target} is not on this machine, so the join link must carry the host's certificate pin. Copy the full link the host shows."), "This join link is incomplete"),
            (format!("the host at {target} did not answer within 10 seconds"), "The host did not answer"),
            ("the host did not answer within 15 seconds".to_string(), "The host did not answer"),
            (format!("could not reach the host at {target}: timed out. Check the address, that the host is running, and that the link is current (each hosting session has a new key and pin)."), "Could not reach the host"),
            ("the host closed the connection: shutting down".to_string(), "The host closed the connection"),
            ("the host removed this player: sent Hello twice".to_string(), "The host removed you"),
            ("could not open a network socket: address in use".to_string(), "This computer could not connect"),
            ("the host sent an unreadable message: bad tag".to_string(), "The host runs a different version"),
            ("something new went wrong".to_string(), "Could not join the session"),
        ];
        for (reason, title) in cases {
            let n = join_failed(&reason, true);
            assert_eq!(n.title, title, "{reason}");
            assert!(n.detail.ends_with('.') && n.detail.chars().next().unwrap().is_uppercase(), "{:?}", n.detail);
        }
        // The unreachable case keeps the address and drops the log's advice, which the hint says.
        let n = join_failed(&format!("could not reach the host at {target}: timed out. Check the address"), true);
        assert_eq!(n.detail, format!("Could not reach the host at {target}: timed out."));
    }

    /// A join that sent a gallery's ticket is never pointed back at the same
    /// link: however it fails, the player joins again from the gallery, and
    /// the host's own words still come through.
    #[test]
    fn a_gallery_join_that_fails_is_joined_again_from_the_gallery() {
        let target = "203.0.113.7:7777";
        let reasons = [
            "the host refused the join: This ticket was already used.".to_string(),
            "the host refused the join: This session is full.".to_string(),
            "the host refused the join: This host speaks protocol 9 and your Player speaks 8.".to_string(),
            format!("the host at {target} did not answer within 10 seconds"),
            "the host did not answer within 15 seconds".to_string(),
            format!("could not reach the host at {target}: timed out. Check the address"),
            format!("this link is from an earlier hosting session: the host at {target} is not the one it names."),
            "could not open a network socket: address in use".to_string(),
            "the host closed the connection: shutting down".to_string(),
            "something new went wrong".to_string(),
        ];
        for reason in &reasons {
            let n = join_failed_with_gallery_ticket(reason);
            assert_eq!(n.hint.as_deref(), Some(GALLERY_REJOIN), "{reason}");
            let usual = join_failed(reason, true);
            assert_eq!((n.title, n.detail), (usual.title, usual.detail), "{reason}");
        }
        let n = join_failed_with_gallery_ticket("the host refused the join: This ticket was already used.");
        assert_eq!(n.detail, "This ticket was already used.");
        // The gallery's own words already say it, so it is not said twice.
        let gallery = "the host refused the join: The gallery could not confirm this join right now. Join again from the gallery.";
        assert_eq!(join_failed_with_gallery_ticket(gallery).hint, None);
        // A player the host removed is not invited back.
        assert_eq!(join_failed_with_gallery_ticket("the host removed this player: sent Hello twice").hint, None);
        assert_eq!(disconnected_with_gallery_ticket("the host removed this player: cheating").hint, None);
        let ended = disconnected_with_gallery_ticket("the host closed the connection: stopped");
        assert_eq!((ended.title.as_str(), ended.hint.as_deref()), ("The session ended", Some(GALLERY_REJOIN)));
    }

    #[test]
    fn a_disconnect_says_how_it_ended() {
        assert_eq!(disconnected("the host removed this player: cheating").title, "The host removed you");
        assert_eq!(disconnected("the host closed the connection: stopped").title, "The session ended");
        let n = disconnected("connection lost");
        assert_eq!((n.title.as_str(), n.detail.as_str()), ("Disconnected from the session", "Connection lost."));
    }

    #[test]
    fn a_download_failure_reads_its_status() {
        let url = "https://api.eustress.dev/api/simulations/ab/world/manifest";
        assert_eq!(fetch_failed(&format!("404 Not Found from {url}: "), true).title, "This simulation was not found");
        let private = fetch_failed(&format!("403 Forbidden from {url}: no"), false);
        assert_eq!(private.title, "This simulation is not open to you");
        assert!(private.hint.unwrap().contains("Sign in"));
        assert_eq!(fetch_failed(&format!("503 Service Unavailable from {url}"), true).title, "Eustress is having trouble");
        assert_eq!(fetch_failed(&format!("GET {url}: error sending request"), true).title, "Could not reach Eustress");
        assert_eq!(fetch_failed("empty .pak", true).title, "The simulation could not be downloaded");
    }

    #[test]
    fn a_log_line_becomes_a_sentence() {
        assert_eq!(sentence("  the host said no "), "The host said no.");
        assert_eq!(sentence("Already one."), "Already one.");
        assert_eq!(sentence(""), "");
        assert_eq!(world_failed("bad chunk").detail, "Bad chunk.");
    }
}
