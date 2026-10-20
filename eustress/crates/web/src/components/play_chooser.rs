// =============================================================================
// Eustress Web - Play: what a listing can do right now, and handing it to the Player
// =============================================================================
// The rules behind every Play button on the site, and the in-place chooser they open. A pure function
// (`panel_for`) decides which Play area a listing gets from its review status, who is looking, whether
// someone hosts it, and the visitor's computer:
//
//   Listed, hosted        Join live session (signed in), or sign in to join
//   Listed, not hosted    Play on your computer ("Offline: nobody is hosting right now")
//   In review             "Your Space is being reviewed ..." and no Play button
//   Held for a person     the same, worded for a person's review
//   Changes requested     the reasons the author may see, and the way back to the review
//   Not listed            the same, for a decision against listing
//
// A listing that is not in the Gallery is the author's to see, so everyone else gets "not found"; a held
// case shows no reason, ever. While a review is open the page asks again every few seconds. Play asks the
// Worker for the link at the click ("live": the host's join link; "solo": this world, played alone) and
// hands it to the Eustress Player; when the browser does not take the link, the chooser says so, with the
// way to install the Player. The first time, the chooser asks how to play (the Eustress Player, recommended,
// or the browser, which says "coming soon" until it exists) and remembers the answer.
// =============================================================================

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;

use crate::api::live::{get_live_host, is_join_link, is_launch_link, LiveHost};
use crate::api::{self, ApiClient, ApiError};
use crate::api::catalog::Listing;
use crate::components::key_prompt::{tab_wraps_to, Place};
use crate::pages::review_panel::{status_label, ReviewView};
use crate::state::{api_name_for_host, AppState, AuthState};
use crate::utils::after_event;

// -----------------------------------------------------------------------------
// 1. What the Worker says
// -----------------------------------------------------------------------------

/// `POST /api/simulations/{id}/play`: how to open the listing.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PlayResponse {
    #[serde(default)]
    status: String,
    #[serde(default)]
    launch: Option<Launch>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Launch {
    /// The link to open: the host's join link when `status` is `live`, otherwise `solo_link`.
    #[serde(default)]
    link: Option<String>,
    /// `eustress-player://play/<id>`: this world, played alone.
    #[serde(default)]
    solo_link: Option<String>,
}

// -----------------------------------------------------------------------------
// 2. What decides the page
// -----------------------------------------------------------------------------

/// A listing's review status, as its author sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Listed,
    Private,
    InReview,
    ChangesRequested,
    NotListed,
    AppealInReview,
    Unreviewed,
}

pub fn status_of(listing: &Listing) -> Status {
    if listing.listable != Some(false) {
        return Status::Listed;
    }
    match listing.moderation.as_ref().map(|m| m.status.as_str()) {
        Some("listed") => Status::Listed,
        Some("approved_private") => Status::Private,
        Some("in_review") => Status::InReview,
        Some("changes_requested") => Status::ChangesRequested,
        Some("not_listed") => Status::NotListed,
        Some("appeal_in_review") => Status::AppealInReview,
        _ => Status::Unreviewed,
    }
}

/// Whether someone hosts the listing now, as far as this visitor may know.
#[derive(Clone, Debug, PartialEq)]
pub enum Live {
    /// No answer yet, or this deployment has no live registry: say nothing about it.
    Unknown,
    Offline,
    /// `joinable`: the Worker gave this visitor the host's link (signed in, or a guest with a lease).
    /// `full`: every server is full, so the page does not ask the visitor to sign in.
    Hosted { players: u32, max_players: u32, joinable: bool, full: bool, servers: Vec<crate::api::live::LiveServer> },
}

impl From<LiveHost> for Live {
    fn from(host: LiveHost) -> Self {
        if !host.live {
            return Live::Offline;
        }
        let servers = crate::api::live::rank_servers(host.servers.clone());
        if host.all_full {
            return Live::Hosted { players: host.players, max_players: host.max_players, joinable: false, full: true, servers };
        }
        match host.join_link() {
            Some(_) => Live::Hosted { players: host.players, max_players: host.max_players, joinable: true, full: false, servers },
            None if host.link_hidden && host.link.is_none() => Live::Hosted { players: host.players, max_players: host.max_players, joinable: false, full: false, servers },
            // Live, with a link this page will not open: say nothing.
            None => Live::Unknown,
        }
    }
}

/// The visitor's computer, as far as Play cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewer {
    Desktop,
    /// Joining a hosted session from the Gallery is Windows and Linux for now.
    Mac,
    /// Eustress Player runs on computers.
    Phone,
}

impl Viewer {
    pub fn from_user_agent(user_agent: &str) -> Self {
        let ua = user_agent.to_ascii_lowercase();
        if ua.contains("android") || ua.contains("iphone") || ua.contains("ipad") {
            Viewer::Phone
        } else if ua.contains("mac os") || ua.contains("macintosh") {
            Viewer::Mac
        } else {
            Viewer::Desktop
        }
    }

    /// The visitor's computer. The prerender has no visitor, so it says desktop.
    pub fn detect() -> Self {
        #[cfg(feature = "ssr")]
        {
            Viewer::Desktop
        }
        #[cfg(not(feature = "ssr"))]
        {
            web_sys::window().and_then(|w| w.navigator().user_agent().ok()).map(|ua| Viewer::from_user_agent(&ua)).unwrap_or(Viewer::Desktop)
        }
    }
}

/// Everything the Play area depends on.
#[derive(Clone, Debug)]
pub struct Facts {
    pub status: Status,
    pub author: bool,
    pub person_review: bool,
    pub live: Live,
    pub viewer: Viewer,
}

/// The Play area's state.
#[derive(Clone, Debug, PartialEq)]
pub enum Panel {
    /// A session is hosted and this visitor may join it.
    Join { players: u32, max_players: u32 },
    /// Every server is full. Play alone is still offered. Signing in would not open a seat.
    Full { players: u32, max_players: u32 },
    /// A session is hosted; joining needs an account.
    SignInToJoin { players: u32, max_players: u32 },
    /// A session is hosted, and this visitor's computer cannot join from the Gallery yet.
    MacCannotJoin { players: u32, max_players: u32 },
    /// Play alone. `hosted` is `Some(false)` when the registry says nobody hosts it.
    Solo { offline: bool, private: bool },
    NeedsComputer,
    InReview { person: bool },
    ChangesRequested,
    NotListed,
    AppealInReview,
    Unreviewed,
}

pub fn panel_for(facts: &Facts) -> Panel {
    match facts.status {
        Status::InReview => Panel::InReview { person: facts.person_review },
        Status::ChangesRequested => Panel::ChangesRequested,
        Status::NotListed => Panel::NotListed,
        Status::AppealInReview => Panel::AppealInReview,
        Status::Unreviewed => Panel::Unreviewed,
        Status::Listed | Status::Private => {
            if facts.viewer == Viewer::Phone {
                return Panel::NeedsComputer;
            }
            match facts.live {
                Live::Hosted { players, max_players, full: true, .. } => Panel::Full { players, max_players },
                Live::Hosted { players, max_players, joinable: true, .. } if facts.viewer == Viewer::Mac => Panel::MacCannotJoin { players, max_players },
                Live::Hosted { players, max_players, joinable: true, .. } => Panel::Join { players, max_players },
                Live::Hosted { players, max_players, joinable: false, .. } => Panel::SignInToJoin { players, max_players },
                Live::Offline => Panel::Solo { offline: true, private: facts.status == Status::Private },
                Live::Unknown => Panel::Solo { offline: false, private: facts.status == Status::Private },
            }
        }
    }
}

/// The words of one Play area: its title and its sentence. The review states take their wording from the Worker
/// (`headline`), which already says only what an author may be told; the one exception is a review that is open,
/// where the page says what it can do about it, which is wait.
pub fn panel_text(panel: &Panel, author: bool, headline: &str) -> (String, String) {
    let listing = if author { "Your Space" } else { "This Space" };
    let headline = |fallback: &str| if headline.is_empty() { fallback.to_string() } else { headline.to_string() };
    match panel {
        Panel::Join { players, max_players } => ("Live now".to_string(), format!("{} are in a session hosted by the author. Join them in Eustress Player.", player_count(*players, *max_players))),
        Panel::Full { players, max_players } => ("Live now: every server is full".to_string(), format!("{} are in a session hosted by the author. There is no open seat. You can play on your own.", player_count(*players, *max_players))),
        Panel::SignInToJoin { players, max_players } => (
            "Live now: sign in to join".to_string(),
            format!("{} are in a session hosted by the author. Joining a live session needs an Eustress account. You can also play on your own, without one.", player_count(*players, *max_players)),
        ),
        Panel::MacCannotJoin { players, max_players } => (
            "Live now".to_string(),
            format!("{} are in a session hosted by the author. Joining a live session from the Gallery works on Windows and Linux for now. You can play on your own on this Mac.", player_count(*players, *max_players)),
        ),
        Panel::Solo { offline, private } => {
            let mut text = String::from("Eustress Player downloads this world once, keeps it cached, and runs it on your computer.");
            if *offline {
                text.push_str(" Nobody is hosting a shared session right now, so you play on your own.");
            }
            if *private {
                text.push_str(" It passed review and is private, so only you can play it.");
            }
            ("Play on your computer".to_string(), text)
        }
        Panel::NeedsComputer => ("Play from a computer".to_string(), "Eustress Player runs on Windows, macOS and Linux. Open this page on a computer to play.".to_string()),
        Panel::InReview { person: false } => (
            "In review".to_string(),
            format!("{listing} is being reviewed. You can play it once it's approved, usually within a minute."),
        ),
        Panel::InReview { person: true } => (
            "Held for a person to review".to_string(),
            format!("A person is reviewing {}. You will hear from us when it is decided. You can play it once it is approved.", listing.to_lowercase()),
        ),
        Panel::ChangesRequested => ("Changes requested".to_string(), headline(&format!("{listing} needs a few changes before it can be listed."))),
        Panel::NotListed => ("Not listed".to_string(), headline(&format!("{listing} was not listed in the Gallery."))),
        Panel::AppealInReview => ("Appeal in review".to_string(), headline("The appeal is with a reviewer.")),
        Panel::Unreviewed => ("Not reviewed yet".to_string(), headline("This listing has not been reviewed yet.")),
    }
}

/// What the author is told when the manifest answers 409: the listing is .echk but its stored world is still a .pak, and the Worker
/// will not serve it until the author publishes once more.
pub const REPUBLISH_NOTE: &str = "Republish once to make your listing playable again.";

/// The manifest's answer that means the author must republish (409 Conflict).
pub fn needs_republish(e: &ApiError) -> bool {
    matches!(e, ApiError::Server { status: 409, .. })
}

pub fn player_count(players: u32, max_players: u32) -> String {
    let noun = if max_players == 1 { "player" } else { "players" };
    format!("{players} of {max_players} {noun}")
}

/// The chip about hosting under the title, once the registry has answered.
pub fn live_chip(live: &Live) -> Option<(&'static str, String)> {
    match live {
        Live::Unknown => None,
        Live::Offline => Some(("lp-chip plain", "Offline: nobody is hosting right now".to_string())),
        Live::Hosted { players, max_players, .. } => Some(("lp-chip live", format!("Live: {}", player_count(*players, *max_players)))),
    }
}

pub fn status_chip_class(status: Status) -> &'static str {
    match status {
        Status::Listed => "lp-chip ok",
        Status::Private => "lp-chip plain",
        Status::InReview | Status::AppealInReview | Status::Unreviewed => "lp-chip info",
        Status::ChangesRequested => "lp-chip warn",
        Status::NotListed => "lp-chip bad",
    }
}

/// The link the page may give to the browser: only the Player's own two links, only the characters their grammar uses. A
/// solo link may end in `?api=production` or `?api=staging` and nothing else; a join link may carry the parameters the
/// chooser appends (`ticket`, `api`), which is why it is checked against the final-link grammar.
pub fn is_player_link(link: &str) -> bool {
    is_solo_link(link) || (link.starts_with("eustress-player://join/") && is_launch_link(link))
}

/// `eustress-player://play/<id>`, with exactly `?api=production` or `?api=staging` after the id, or nothing.
fn is_solo_link(link: &str) -> bool {
    let Some(rest) = link.strip_prefix("eustress-player://play/") else { return false };
    let id = match rest.split_once('?') {
        None => rest,
        Some((id, "api=production" | "api=staging")) => id,
        Some(_) => return false,
    };
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// A link as the Worker sends it, before the chooser appends anything: a bare `play/<id>` or a join link of at most 512
/// characters (`is_join_link`).
fn is_worker_link(link: &str) -> bool {
    let play = link.strip_prefix("eustress-player://play/").map(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')).unwrap_or(false);
    play || is_join_link(link)
}

/// Which link Play opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The host's session.
    Join,
    /// This world, alone.
    Solo,
}

/// The link for `kind` from the Worker's answer, or the sentence that says why there is none.
pub fn chosen_link(answer: &PlayResponse, kind: Kind) -> Result<String, &'static str> {
    let launch = answer.launch.clone().unwrap_or_default();
    let link = match kind {
        Kind::Join if answer.status == "live" => launch.link,
        Kind::Join => return Err("The live session just ended. Press Play to play on your own, or check back in a moment."),
        Kind::Solo => launch.solo_link.or(launch.link.filter(|l| l.starts_with("eustress-player://play/"))),
    };
    match link {
        Some(l) if is_worker_link(&l) => Ok(l),
        Some(_) => Err("Eustress sent a link this page will not open."),
        None => Err("Eustress sent no link to open."),
    }
}

/// How long until the page asks again: a review that is open every few seconds, a held one more slowly, hosting every
/// half minute; a decided review nothing, since only the author changes it.
pub fn poll_interval_ms(status: Status, person_review: bool) -> Option<f64> {
    match status {
        Status::InReview | Status::AppealInReview if person_review => Some(30_000.0),
        Status::InReview | Status::AppealInReview => Some(6_000.0),
        Status::Listed | Status::Private => Some(30_000.0),
        _ => None,
    }
}

// -----------------------------------------------------------------------------
// 3. Fetching and watching
// -----------------------------------------------------------------------------

/// How often the poll wakes. A tab in the background asks nothing, and asks again within this long of being shown.
pub const TICK_MS: u32 = 2_000;

/// How often an open listing whose image URLs are signed is read again. The Worker signs an adult or unrated listing's thumbnail, cover
/// and preview URLs for 15 to 30 minutes, so a page left open re-reads well before they lapse.
pub const RESIGN_MS: f64 = 10.0 * 60_000.0;

/// Whether the Worker signs this listing's image URLs: adult (18+) or unrated, that is anything outside the three ratings below.
pub fn signed_images(listing: &Listing) -> bool {
    !matches!(listing.rating.as_deref(), Some("all_ages") | Some("ages_10") | Some("teen_13") | Some("mature_17"))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Load {
    Loading,
    /// Not there, or not the visitor's to see.
    Missing,
    /// Adult content (rated 18+) and the viewer's Adult content setting is off: the Worker's 403 { code: "adult_content_off" }.
    AdultOff { signed_in: bool },
    /// A 21+ listing and the viewer is under 21 (403 `age_21_required`): the Worker's words.
    Age21Required(String),
    Failed(String),
    Ready(Listing),
}

pub fn page_hidden() -> bool {
    web_sys::window().and_then(|w| w.document()).is_some_and(|d| d.hidden())
}

/// The reader's clock in milliseconds. The prerender has no reader, so it uses the machine's.
pub fn now_ms() -> f64 {
    #[cfg(feature = "ssr")]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
    }
    #[cfg(not(feature = "ssr"))]
    {
        js_sys::Date::now()
    }
}

pub async fn fetch_listing(client: &ApiClient, id: &str) -> Load {
    load_of(client.get::<Listing>(&format!("/api/simulations/{id}")).await)
}

/// What a listing read's answer shows. An adult listing held back by the viewer's setting gets its own notice; any other 403 reads
/// as missing, so a private listing is not told apart from one that does not exist.
pub fn load_of(answer: Result<Listing, ApiError>) -> Load {
    match answer {
        Ok(listing) => Load::Ready(listing),
        Err(ApiError::NotFound) | Err(ApiError::Unauthorized) => Load::Missing,
        Err(e) => match (crate::components::adult_content::age_21_required(&e), crate::components::adult_content::adult_off(&e)) {
            (Some(message), _) => Load::Age21Required(message),
            (None, Some(signed_in)) => Load::AdultOff { signed_in },
            (None, None) if matches!(e, ApiError::Server { status: 403, .. }) => Load::Missing,
            (None, None) => Load::Failed(api::failure_sentence(&e)),
        },
    }
}

/// Load the listing, then keep it and its host current until the page is left or shows another listing. Each start
/// takes a new `run` number; an older loop sees the change and stops, and leaving the page disposes `runs`.
pub async fn watch(api_url: String, id: String, load: RwSignal<Load>, live: RwSignal<Live>, runs: StoredValue<u64>, run: u64) {
    let client = ApiClient::new(&api_url);
    let current = move || runs.try_get_value() == Some(run);
    let first = fetch_listing(&client, &id).await;
    if !current() {
        return;
    }
    load.set(first);
    let mut last_listing = now_ms();
    let mut last_live: Option<f64> = None;
    let mut was_hidden = false;
    loop {
        TimeoutFuture::new(TICK_MS).await;
        if !current() {
            return;
        }
        let hidden = page_hidden();
        let shown_again = was_hidden && !hidden;
        was_hidden = hidden;
        if hidden {
            continue;
        }
        let now = now_ms();
        match load.get_untracked() {
            // A dropped connection is tried again.
            Load::Failed(_) => {
                if now - last_listing >= 15_000.0 {
                    last_listing = now;
                    let answer = fetch_listing(&client, &id).await;
                    if current() {
                        load.set(answer);
                    }
                }
            }
            Load::Ready(listing) => {
                let status = status_of(&listing);
                let person = listing.moderation.as_ref().is_some_and(|m| m.person_review);
                match status {
                    Status::InReview | Status::AppealInReview => {
                        let every = poll_interval_ms(status, person).unwrap_or(30_000.0);
                        if shown_again || now - last_listing >= every {
                            last_listing = now;
                            let answer = fetch_listing(&client, &id).await;
                            if !current() {
                                return;
                            }
                            if let Load::Ready(next) = &answer {
                                // Approved while the page was open: the Play area replaces the wait, and asks about hosting at once.
                                if status_of(next) != status {
                                    last_live = None;
                                    live.set(Live::Unknown);
                                }
                            }
                            if answer != Load::Ready(listing.clone()) {
                                load.set(answer);
                            }
                        }
                    }
                    Status::Listed | Status::Private => {
                        // Fresh image URLs before the signed ones lapse. A dropped connection keeps the page as it is.
                        if signed_images(&listing) && (shown_again || now - last_listing >= RESIGN_MS) {
                            last_listing = now;
                            let answer = fetch_listing(&client, &id).await;
                            if !current() {
                                return;
                            }
                            if !matches!(answer, Load::Failed(_)) && answer != Load::Ready(listing.clone()) {
                                load.set(answer);
                            }
                        }
                        if shown_again || last_live.map_or(true, |at| now - at >= 30_000.0) {
                            last_live = Some(now);
                            match get_live_host(&client, &id).await {
                                Ok(host) => {
                                    if current() {
                                        live.set(Live::from(host));
                                    }
                                }
                                // No live registry on this deployment, or the listing is gone: asking again changes nothing.
                                Err(ApiError::NotFound) | Err(ApiError::Server { status: 503, .. }) => {
                                    if current() {
                                        live.set(Live::Unknown);
                                    }
                                    last_live = Some(f64::MAX / 2.0);
                                }
                                Err(_) => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            Load::Loading | Load::Missing | Load::AdultOff { .. } | Load::Age21Required(_) => {}
        }
    }
}

// -----------------------------------------------------------------------------
// 4. Handing the link to the Player
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Handoff {
    Idle,
    /// Asking the Worker for the link.
    Asking,
    /// The link went to the browser; the Player may be opening.
    Opening { link: String },
    /// The browser did not take it: the Player is not installed, or not registered yet.
    NotOpened { link: String },
    Failed(String),
}

/// Give a link to the browser, which opens the Player where it is registered. Call it from a click: browsers open another
/// application's link only in answer to one.
pub fn open_player(link: &str) {
    if let Some(window) = web_sys::window() {
        let _ = window.location().set_href(link);
    }
}

/// Whether the browser or the Player took the focus: the browser is asking whether to open the Player, or the Player is up.
pub fn focus_left_the_page() -> bool {
    web_sys::window().and_then(|w| w.document()).is_none_or(|d| d.hidden() || !d.has_focus().unwrap_or(true))
}

/// After a link was handed over: if the page still has the focus a moment later, nothing opened.
pub async fn watch_launch(handoff: RwSignal<Handoff>, link: String) {
    for _ in 0..10 {
        TimeoutFuture::new(250).await;
        if focus_left_the_page() {
            return;
        }
    }
    if handoff.try_get_untracked().is_some_and(|h| h == Handoff::Opening { link: link.clone() }) {
        handoff.set(Handoff::NotOpened { link });
    }
}

// -----------------------------------------------------------------------------
// 4a. The join ticket
// -----------------------------------------------------------------------------
// A signed-in viewer who presses Play on a live session gets a short-lived sign-in for the host (docs/networking/
// PLAYER_JOIN_TICKET.md). The ticket exists in one place: the string given to `open_player`. It is never kept in a
// signal, a stored value or `Handoff`, in storage or history, in the page, in a log line or in an error sentence, and
// never goes into an `http(s)` URL. Whatever the page keeps or shows is the link without it.

/// How long minting may take. The click's user activation lasts about 5 seconds and `/play` has already spent part of it.
const TICKET_BUDGET_MS: u32 = 2_500;

/// The longest ticket the page will pass on.
const TICKET_MAX_LEN: usize = 2_048;

/// `POST /api/identity/ticket` answers `{ ticket, expires, id }`. No `Debug`, so nothing can print the ticket.
#[derive(Deserialize)]
struct TicketAnswer {
    ticket: String,
}

/// A ticket the page will append: 1 to 2,048 characters of `A-Z a-z 0-9 . _ -`.
pub fn is_ticket(ticket: &str) -> bool {
    !ticket.is_empty() && ticket.len() <= TICKET_MAX_LEN && ticket.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// `link` with the API name appended: `&api=<name>` after a query, `?api=<name>` without one.
pub fn with_api(link: &str, api: &str) -> String {
    let separator = if link.contains('?') { '&' } else { '?' };
    format!("{link}{separator}api={api}")
}

/// `link` with `&ticket=<ticket>&api=<name>` appended. A ticket that is empty, longer than 2,048 characters or has a
/// character outside `A-Z a-z 0-9 . _ -` is refused: the link comes back with only the API name.
pub fn with_ticket(link: &str, ticket: &str, api: &str) -> String {
    if !is_ticket(ticket) {
        return with_api(link, api);
    }
    let separator = if link.contains('?') { '&' } else { '?' };
    format!("{link}{separator}ticket={ticket}&api={api}")
}

/// The link to give the browser: with the ticket when there is one, with the API name alone otherwise. A result that is
/// not a link the Player's grammar allows falls back to the one without the ticket.
pub fn launch_link(link: &str, ticket: Option<&str>, api: &str) -> String {
    match ticket {
        Some(ticket) => {
            let full = with_ticket(link, ticket, api);
            if is_launch_link(&full) {
                full
            } else {
                with_api(link, api)
            }
        }
        // A solo link carries `api` on staging only. Staging is tested with a Player that reads it (build 32, P1); production keeps
        // the link as the Worker sent it until a Player release that reads it ships, because an older Player takes the whole query
        // as part of the listing id, refuses it, and exits without a window. Join links carry both (an old Player ignores
        // parameters it does not know).
        None if link.starts_with("eustress-player://play/") && api == "staging" => with_api(link, api),
        None if link.starts_with("eustress-player://play/") => link.to_string(),
        None => with_api(link, api),
    }
}

/// The pin (64 hex characters) in a join link's query: the audience a ticket is minted for.
pub fn pin_of(link: &str) -> Option<&str> {
    let (_, query) = link.split_once('?')?;
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("pin="))
        .filter(|pin| pin.len() == 64 && pin.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The pin to mint a ticket for: only when joining a live session, only for a signed-in viewer, only when the host's
/// link carries a pin.
pub fn ticket_audience(kind: Kind, signed_in: bool, link: &str) -> Option<&str> {
    if kind == Kind::Join && signed_in {
        pin_of(link)
    } else {
        None
    }
}

/// The API the Player should use, named by the site's own host.
fn site_api_name() -> &'static str {
    let host = web_sys::window().and_then(|w| w.location().hostname().ok()).unwrap_or_default();
    api_name_for_host(&host)
}

/// Ask for a ticket for `pin`, with the signed-in viewer's session, within the budget. A timeout or any failure is `None`:
/// the link then opens without a ticket. The failure is dropped on purpose, not logged or shown.
async fn mint_ticket(api_url: &str, pin: &str) -> Option<String> {
    let client = ApiClient::new(api_url);
    let body = serde_json::json!({ "audience": pin });
    let ask = client.post::<TicketAnswer, _>("/api/identity/ticket", &body);
    futures::pin_mut!(ask);
    match futures::future::select(ask, TimeoutFuture::new(TICKET_BUDGET_MS)).await {
        futures::future::Either::Left((answer, _)) => answer.ok().map(|answer| answer.ticket),
        futures::future::Either::Right(_) => None,
    }
}

// -----------------------------------------------------------------------------
// 5. The page
// -----------------------------------------------------------------------------

pub fn icon(name: &str) -> AnyView {
    match name {
        "play" => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5.5v13l11-6.5z"/></svg> }.into_any(),
        "users" => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="9" cy="8" r="3.2"/><path d="M3 19c.6-3.2 3-5 6-5s5.4 1.8 6 5"/><path d="M16 5.5a3 3 0 0 1 0 5.8M18 14.2c1.6.7 2.6 2.2 3 4.8"/></svg> }.into_any(),
        "hourglass" => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 3h10M7 21h10M8 3c0 4 4 5 4 9s-4 5-4 9M16 3c0 4-4 5-4 9s4 5 4 9"/></svg> }.into_any(),
        "person" => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="8" r="3.5"/><path d="M5 20c.8-4 3.4-6 7-6s6.2 2 7 6"/></svg> }.into_any(),
        "edit" => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 20l4-1 11-11-3-3L5 16l-1 4z"/><path d="M14 6l3 3"/></svg> }.into_any(),
        "block" => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M6 6l12 12"/></svg> }.into_any(),
        "monitor" => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="4" width="18" height="12" rx="1.5"/><path d="M8 20h8M12 16v4"/></svg> }.into_any(),
        "globe" => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c3 3.2 3 14.8 0 18M12 3c-3 3.2-3 14.8 0 18"/></svg> }.into_any(),
        "alert" => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3.5l9.5 16.5h-19L12 3.5z"/><path d="M12 10v4.5M12 17.5h.01"/></svg> }.into_any(),
        "cube" => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3l8 4.5v9L12 21l-8-4.5v-9L12 3z"/><path d="M12 12l8-4.5M12 12L4 7.5M12 12v9"/></svg> }.into_any(),
        _ => ().into_any(),
    }
}

pub fn panel_icon(panel: &Panel) -> (&'static str, &'static str) {
    match panel {
        Panel::Join { .. } | Panel::SignInToJoin { .. } | Panel::MacCannotJoin { .. } | Panel::Full { .. } => ("lp-icon live", "users"),
        Panel::Solo { .. } => ("lp-icon ok", "play"),
        Panel::NeedsComputer => ("lp-icon", "monitor"),
        Panel::InReview { person: false } => ("lp-icon info", "hourglass"),
        Panel::InReview { person: true } => ("lp-icon info", "person"),
        Panel::ChangesRequested => ("lp-icon warn", "edit"),
        Panel::NotListed => ("lp-icon bad", "block"),
        Panel::AppealInReview => ("lp-icon info", "person"),
        Panel::Unreviewed => ("lp-icon", "hourglass"),
    }
}

/// The steps a person follows when the Player does not open.
#[component]
pub fn PlayerHelp(#[prop(optional)] open: bool) -> impl IntoView {
    view! {
        <details class="lp-help" open=open>
            <summary>"Eustress Player not opening?"</summary>
            <ol>
                <li>"Download Eustress Player and run it once. That registers the eustress-player link with your browser."</li>
                <li>"Press Play again. Chrome or Edge asks once whether to open Eustress Player: allow it."</li>
                <li>"If you pressed Play before the Player had run, reload this page first, then press Play."</li>
                <li>"If you have more than one Player build, the last one you ran is the one that opens."</li>
            </ol>
        </details>
    }
}

// -----------------------------------------------------------------------------
// 6. How the visitor chose to play
// -----------------------------------------------------------------------------

/// How the visitor plays, remembered in this browser. Today there is one way; the browser Player is not built, so the chooser
/// shows it as "coming soon" and never stores it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Player,
}

pub const MODE_KEY: &str = "eustress.play_mode";

pub fn mode_from_stored(value: Option<&str>) -> Option<Mode> {
    match value {
        Some("player") => Some(Mode::Player),
        _ => None,
    }
}

pub fn read_mode() -> Option<Mode> {
    use gloo_storage::{LocalStorage, Storage};
    mode_from_stored(LocalStorage::get::<String>(MODE_KEY).ok().as_deref())
}

pub fn save_mode(mode: Option<Mode>) {
    use gloo_storage::{LocalStorage, Storage};
    match mode {
        Some(Mode::Player) => {
            let _ = LocalStorage::set(MODE_KEY, "player");
        }
        None => LocalStorage::delete(MODE_KEY),
    }
}

// -----------------------------------------------------------------------------
// 7. The chooser
// -----------------------------------------------------------------------------

/// The Play chooser for one listing, as a dialog over whatever page the Play button is on: how to play (asked once), then the Play
/// area for the listing's state. It reads the listing and keeps it current while it is open, so a review that finishes while the
/// chooser is up turns the wait into the Play button by itself.
#[component]
pub fn PlayChooser(#[prop(into)] sim_id: String, on_close: Callback<()>) -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    let api_url = StoredValue::new(app_state.api_url.clone());
    let load = RwSignal::new(Load::Loading);
    let live = RwSignal::new(Live::Unknown);
    let handoff = RwSignal::new(Handoff::Idle);
    let mode = RwSignal::new(read_mode());
    let runs = StoredValue::new(1u64);
    let viewer = Viewer::detect();
    let dialog = NodeRef::<leptos::html::Div>::new();
    let close_button = NodeRef::<leptos::html::Button>::new();

    // Load once; the loop ends when the dialog is closed and `runs` is disposed with it.
    {
        let sim = sim_id.clone();
        Effect::new(move |_| {
            spawn_local(watch(api_url.get_value(), sim.clone(), load, live, runs, 1));
        });
    }

    // The dialog takes the focus when it opens, so a screen reader reads it and Tab goes to its first control; once the listing is
    // there, the focus moves to the main action so Enter completes what the Play button started.
    Effect::new(move |_| {
        if let Some(el) = dialog.get() {
            let _ = el.focus();
        }
    });
    Effect::new(move |_| {
        let ready = matches!(load.get(), Load::Ready(_));
        let _ = handoff.get();
        let _ = mode.get();
        if !ready {
            return;
        }
        after_event(move || {
            let main = dialog.get_untracked().and_then(|d| d.query_selector(".btn-primary").ok().flatten());
            if let Some(main) = main.and_then(|m| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(m).ok()) {
                let _ = main.focus();
            }
        });
    });

    // Closing waits for the click to finish: the click is still travelling through elements this removes.
    let close = move || after_event(move || on_close.run(()));

    let is_author = move |listing: &Listing| match auth.get() {
        AuthState::Authenticated(user) => user.id.to_string().eq_ignore_ascii_case(&listing.author_id),
        _ => false,
    };

    // An author's .echk listing whose stored world is still a .pak answers 409 at its manifest until it is published again. Ask once
    // when the author opens the chooser, so they are told here instead of meeting a broken link in the Player.
    let republish = RwSignal::new(false);
    let probed = StoredValue::new(false);
    {
        let sim = sim_id.clone();
        Effect::new(move |_| {
            let Load::Ready(listing) = load.get() else { return };
            if probed.get_value() || !is_author(&listing) || listing.format.as_deref() != Some("echk") {
                return;
            }
            probed.set_value(true);
            let sim = sim.clone();
            spawn_local(async move {
                let client = ApiClient::new(&api_url.get_value());
                if let Err(e) = client.get::<serde_json::Value>(&format!("/api/simulations/{sim}/world/manifest")).await {
                    if needs_republish(&e) {
                        republish.set(true);
                    }
                }
            });
        });
    }

    // Play: ask the Worker for the link at the click, then hand it to the browser. Choosing to play remembers the way.
    let play_sim = sim_id.clone();
    let play = Callback::new(move |kind: Kind| {
        if handoff.get_untracked() == Handoff::Asking {
            return;
        }
        if mode.get_untracked().is_none() {
            save_mode(Some(Mode::Player));
            mode.set(Some(Mode::Player));
        }
        handoff.set(Handoff::Asking);
        let sim = play_sim.clone();
        let signed_in = auth.get_untracked().is_authenticated();
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            let answer = client.post::<PlayResponse, _>(&format!("/api/simulations/{sim}/play"), &serde_json::json!({})).await;
            match answer {
                Ok(answer) => match chosen_link(&answer, kind) {
                    Ok(link) => {
                        let api = site_api_name();
                        let shown = launch_link(&link, None, api);
                        {
                            let ticket = match ticket_audience(kind, signed_in, &link) {
                                Some(pin) => mint_ticket(&api_url.get_value(), pin).await,
                                None => None,
                            };
                            let ticketed_link = launch_link(&link, ticket.as_deref(), api);
                            handoff.set(Handoff::Opening { link: shown.clone() });
                            open_player(&ticketed_link);
                        }
                        watch_launch(handoff, shown).await;
                    }
                    Err(why) => handoff.set(Handoff::Failed(why.to_string())),
                },
                Err(e) => handoff.set(Handoff::Failed(api::failure_sentence(&e))),
            }
        });
    });

    // Escape closes. Tab stays inside: the last control is the close button (last in the dialog's order, first on screen by CSS).
    let keys = move |ev: leptos::ev::KeyboardEvent| match ev.key().as_str() {
        "Escape" => {
            ev.prevent_default();
            close();
        }
        "Tab" => {
            let (Some(root), Some(last)) = (dialog.get_untracked(), close_button.get_untracked()) else { return };
            let first = root.query_selector("button:not([disabled]), a[href]").ok().flatten();
            let active = web_sys::window().and_then(|w| w.document()).and_then(|d| d.active_element());
            let last_el: &web_sys::Element = last.as_ref();
            let root_el: &web_sys::Element = root.as_ref();
            let at = match &active {
                Some(el) if el == last_el => Place::Last,
                Some(el) if first.as_ref() == Some(el) => Place::First,
                Some(el) if el == root_el => Place::Dialog,
                _ => Place::Between,
            };
            match tab_wraps_to(at, ev.shift_key()) {
                Some(Place::Last) => {
                    ev.prevent_default();
                    let _ = last.focus();
                }
                Some(Place::First) => {
                    ev.prevent_default();
                    if let Some(first) = first.and_then(|f| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(f).ok()) {
                        let _ = first.focus();
                    }
                }
                _ => {}
            }
        }
        _ => {}
    };

    view! {
        <div class="pc-scrim" on:click=move |ev| { if ev.target() == ev.current_target() { close(); } }>
            <div class="pc" role="dialog" aria-modal="true" aria-labelledby="pc-title" tabindex="-1" node_ref=dialog on:keydown=keys>
                {move || match load.get() {
                    Load::Loading => view! {
                        <div class="pc-body" role="status" aria-busy="true">
                            <h2 id="pc-title" class="pc-title">"Play"</h2>
                            <div class="pc-skeleton"></div>
                            <div class="pc-skeleton short"></div>
                        </div>
                    }.into_any(),
                    Load::Missing => view! {
                        <div class="pc-body" role="status">
                            <h2 id="pc-title" class="pc-title">"We can't find this Space"</h2>
                            <p>"It may have been removed, or it is not public yet. If it is yours, sign in to see it."</p>
                            <div class="lp-actions">
                                {move || (!matches!(auth.get(), AuthState::Authenticated(_))).then(|| view! { <a href="/login" class="btn btn-primary">"Sign in"</a> })}
                                <a href="/gallery" class="btn btn-secondary">"Go to the Gallery"</a>
                            </div>
                        </div>
                    }.into_any(),
                    Load::AdultOff { signed_in } => view! {
                        <div class="pc-body" role="status">
                            <h2 id="pc-title" class="pc-title">"Adult content"</h2>
                            <p>{crate::components::adult_content::OFF_NOTICE}</p>
                            <div class="lp-actions">
                                {if signed_in {
                                    view! { <a href=crate::components::adult_content::SETTING_HREF class="btn btn-primary">"Open the setting"</a> }.into_any()
                                } else {
                                    view! { <a href="/login" class="btn btn-primary">"Sign in"</a> }.into_any()
                                }}
                                <a href="/gallery" class="btn btn-secondary">"Go to the Gallery"</a>
                            </div>
                        </div>
                    }.into_any(),
                    Load::Age21Required(message) => view! {
                        <div class="pc-body" role="status">
                            <h2 id="pc-title" class="pc-title">"Adults only (21+)"</h2>
                            <p>{message}</p>
                            <div class="lp-actions">
                                <a href="/gallery" class="btn btn-secondary">"Go to the Gallery"</a>
                            </div>
                        </div>
                    }.into_any(),
                    Load::Failed(why) => view! {
                        <div class="pc-body" role="alert">
                            <h2 id="pc-title" class="pc-title">"Could not load this Space"</h2>
                            <p>{why}</p>
                            <p class="lp-note">"The chooser tries again by itself."</p>
                        </div>
                    }.into_any(),
                    Load::Ready(listing) => {
                        let author = is_author(&listing);
                        let status = status_of(&listing);
                        let moderation = listing.moderation.clone().unwrap_or_default();
                        let headline = moderation.headline.clone();
                        let reasons = moderation.reasons.clone();
                        let suggested = moderation.suggested_edit.clone();
                        let note = moderation.note.clone();
                        let person_review = moderation.person_review;
                        let listing_id = listing.id.clone();
                        let name = if listing.name.is_empty() { "Untitled Space".to_string() } else { listing.name.clone() };
                        let playable = matches!(status, Status::Listed | Status::Private);
                        let status_name = status_label(match status {
                            Status::Listed => "listed",
                            Status::Private => "approved_private",
                            Status::InReview => "in_review",
                            Status::ChangesRequested => "changes_requested",
                            Status::NotListed => "not_listed",
                            Status::AppealInReview => "appeal_in_review",
                            Status::Unreviewed => "unreviewed",
                        });
                        let review_href = format!("/projects?review={listing_id}");
                        view! {
                            <div class="pc-body">
                                <p class="pc-kicker">"Play"</p>
                                <h2 id="pc-title" class="pc-title">{name.clone()}</h2>
                                <div class="pc-chips">
                                    {(status != Status::Listed).then(|| view! { <span class=status_chip_class(status)>{status_name}</span> })}
                                    {move || live_chip(&live.get()).filter(|_| playable).map(|(class, text)| view! { <span class=class>{text}</span> })}
                                </div>
                                {
                                    let studio = format!("eustress://edit/{listing_id}");
                                    move || (author && republish.get()).then(|| view! {
                                        <div class="lp-panel lp-republish" role="alert">
                                            <div class="lp-panel-body">
                                                <h3>{REPUBLISH_NOTE}</h3>
                                                <p>"Open it in Studio and publish it again. Nothing about it changes; the Gallery gets its world in the current format."</p>
                                                <div class="lp-actions"><a href=studio.clone() class="btn btn-secondary">"Open in Studio"</a></div>
                                            </div>
                                        </div>
                                    })
                                }

                                {move || match handoff.get() {
                                    Handoff::Asking => view! {
                                        <div class="lp-panel">
                                            <div class="lp-icon"><div class="lp-spinner" aria-hidden="true"></div></div>
                                            <div class="lp-panel-body"><h3>"Getting your link..."</h3><p>"One moment."</p></div>
                                        </div>
                                    }.into_any(),
                                    Handoff::Opening { link } => view! {
                                        <div class="lp-panel">
                                            <div class="lp-icon ok">{icon("play")}</div>
                                            <div class="lp-panel-body">
                                                <h3>"Opening Eustress Player..."</h3>
                                                <p>"If your browser asks, allow it to open Eustress Player."</p>
                                                <div class="lp-actions">
                                                    <a href=link class="btn btn-secondary">"Open it again"</a>
                                                    <button class="btn btn-ghost" on:click=move |_| handoff.set(Handoff::Idle)>"Back"</button>
                                                </div>
                                            </div>
                                        </div>
                                        <PlayerHelp />
                                    }.into_any(),
                                    Handoff::NotOpened { link } => view! {
                                        <div class="lp-panel">
                                            <div class="lp-icon warn">{icon("alert")}</div>
                                            <div class="lp-panel-body">
                                                <h3>"Eustress Player didn't open"</h3>
                                                <p>"Eustress Player isn't installed or isn't registered with your browser. Download it, run it once, then press Play again."</p>
                                                <div class="lp-actions">
                                                    <a href="/downloads/player" class="btn btn-primary">"Download Eustress Player"</a>
                                                    <a href=link class="btn btn-secondary">"Try again"</a>
                                                    <button class="btn btn-ghost" on:click=move |_| handoff.set(Handoff::Idle)>"Back"</button>
                                                </div>
                                            </div>
                                        </div>
                                        <PlayerHelp open=true />
                                    }.into_any(),
                                    Handoff::Failed(why) => view! {
                                        <div class="lp-panel">
                                            <div class="lp-icon bad">{icon("alert")}</div>
                                            <div class="lp-panel-body" role="alert">
                                                <h3>"Play could not start"</h3>
                                                <p>{why}</p>
                                                <div class="lp-actions"><button class="btn btn-secondary" on:click=move |_| handoff.set(Handoff::Idle)>"Back"</button></div>
                                            </div>
                                        </div>
                                    }.into_any(),
                                    Handoff::Idle => {
                                        let headline = headline.clone();
                                        let reasons = reasons.clone();
                                        let suggested = suggested.clone();
                                        let note = note.clone();
                                        let review_href = review_href.clone();
                                        view! {
                                            // The first time: how to play. After it, one line with the way to change it.
                                            {move || (playable && mode.get().is_none() && viewer != Viewer::Phone).then(|| view! {
                                                <div class="pc-mode">
                                                    <h3>"How do you want to play?"</h3>
                                                    <div class="pc-opt rec">
                                                        <div class="pc-ico">{icon("monitor")}</div>
                                                        <div>
                                                            <b>"Eustress Player " <span class="lp-chip ok">"Recommended"</span></b>
                                                            <p>"Runs the Space on your computer and can join live sessions. You install it once."</p>
                                                            <div class="lp-actions">
                                                                <button class="btn btn-primary" on:click=move |_| { save_mode(Some(Mode::Player)); mode.set(Some(Mode::Player)); }>"Use Eustress Player"</button>
                                                                <a href="/downloads/player" class="btn btn-secondary">"Download Eustress Player"</a>
                                                            </div>
                                                        </div>
                                                    </div>
                                                    <div class="pc-opt off" aria-disabled="true">
                                                        <div class="pc-ico">{icon("globe")}</div>
                                                        <div>
                                                            <b>"In your browser " <span class="lp-chip plain">"Coming soon"</span></b>
                                                            <p>"Play without installing anything, in your browser. Not available yet."</p>
                                                        </div>
                                                    </div>
                                                </div>
                                            })}
                                            {move || (playable && mode.get().is_some()).then(|| view! {
                                                <p class="pc-modeline">
                                                    "Playing with Eustress Player. "
                                                    <button class="pc-link" on:click=move |_| { save_mode(None); mode.set(None); }>"Change"</button>
                                                </p>
                                            })}
                                            {move || {
                                                let hidden = playable && mode.get().is_none() && viewer != Viewer::Phone;
                                                (!hidden).then(|| {
                                                    let facts = Facts { status, author, person_review, live: live.get(), viewer };
                                                    view! { <PlayStatePanel facts=facts headline=headline.clone() reasons=reasons.clone() suggested=suggested.clone() note=note.clone() review_href=review_href.clone() on_play=play /> }
                                                })
                                            }}
                                        }.into_any()
                                    }
                                }}
                            </div>
                        }.into_any()
                    }
                }}
                <button class="pc-close" node_ref=close_button aria-label="Close" on:click=move |_| close()>
                    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"/></svg>
                </button>
            </div>
        </div>
    }
}

/// The Play area for one state of one listing: what it says, the reasons an author may be told, and the buttons that fit. The
/// chooser and the listing page both draw it; `on_play` is what a Play button does there (the chooser starts the Player, the page
/// opens the chooser).
#[component]
pub fn PlayStatePanel(
    facts: Facts,
    headline: String,
    reasons: Vec<crate::pages::review_panel::ReviewReason>,
    suggested: Option<String>,
    note: Option<String>,
    review_href: String,
    on_play: Callback<Kind>,
) -> impl IntoView {
    let author = facts.author;
    let panel = panel_for(&facts);
    let (title, text) = panel_text(&panel, author, &headline);
    let (icon_class, icon_name) = panel_icon(&panel);
    let busy_review = matches!(panel, Panel::InReview { .. } | Panel::AppealInReview);
    let decided = matches!(panel, Panel::ChangesRequested | Panel::NotListed);
    let actions = match panel.clone() {
        Panel::Full { .. } => view! {
            <button class="btn btn-primary btn-lg" on:click=move |_| on_play.run(Kind::Solo)>"Play on your own"</button>
        }.into_any(),
        Panel::Join { .. } => view! {
            <button class="btn btn-primary btn-lg" on:click=move |_| on_play.run(Kind::Join)>"Join live session"</button>
            <button class="btn btn-secondary" on:click=move |_| on_play.run(Kind::Solo)>"Play on your own"</button>
        }.into_any(),
        Panel::SignInToJoin { .. } => view! {
            <a href="/login" class="btn btn-primary btn-lg">"Sign in to join"</a>
            <button class="btn btn-secondary" on:click=move |_| on_play.run(Kind::Solo)>"Play on your own"</button>
        }.into_any(),
        Panel::MacCannotJoin { .. } | Panel::Solo { .. } => view! {
            <button class="btn btn-primary btn-lg" on:click=move |_| on_play.run(Kind::Solo)>"Play on your computer"</button>
        }.into_any(),
        Panel::NeedsComputer => view! { <a href="/downloads/player" class="btn btn-primary btn-lg">"See the Player downloads"</a> }.into_any(),
        Panel::ChangesRequested | Panel::NotListed | Panel::AppealInReview | Panel::Unreviewed if author => view! {
            <a href=review_href class="btn btn-primary btn-lg">{if decided { "Open the review" } else { "Open the review status" }}</a>
        }.into_any(),
        _ => ().into_any(),
    };
    view! {
        <div class="lp-panel">
            <div class=icon_class>{icon(icon_name)}</div>
            <div class="lp-panel-body">
                <h3>{title}</h3>
                <p>{text}</p>
                {busy_review.then(|| view! { <p class="lp-note"><span class="lp-spinner small" aria-hidden="true"></span>"This updates by itself."</p> })}
                {decided.then(|| view! { <ReviewReasons reasons=reasons.clone() suggested=suggested.clone() note=note.clone() /> })}
                <div class="lp-actions">{actions}</div>
                {matches!(panel, Panel::Solo { .. } | Panel::Join { .. } | Panel::SignInToJoin { .. } | Panel::MacCannotJoin { .. } | Panel::Full { .. }).then(|| view! { <PlayerHelp /> })}
            </div>
        </div>
    }
}

/// Why a decision was made and what to change, as the Worker words it for the author: each reason with its why and its change,
/// then the suggested edit and the reviewer's note. Never shown for a case a person holds; the caller decides when.
#[component]
pub fn ReviewReasons(reasons: Vec<crate::pages::review_panel::ReviewReason>, suggested: Option<String>, note: Option<String>) -> impl IntoView {
    view! {
        {(!reasons.is_empty()).then(|| view! {
            <div class="lp-reasons">
                {reasons.iter().map(|r| view! {
                    <div class="lp-reason">
                        <h4>{r.title.clone()}</h4>
                        {(!r.why.is_empty()).then(|| view! { <p><span class="lp-label">"Why"</span>{r.why.clone()}</p> })}
                        {(!r.what_to_change.is_empty()).then(|| view! { <p><span class="lp-label">"What to change"</span>{r.what_to_change.clone()}</p> })}
                    </div>
                }).collect_view()}
            </div>
        })}
        {suggested.map(|s| view! { <p class="lp-suggest"><span class="lp-label">"Suggested edit"</span>{s}</p> })}
        {note.map(|n| view! { <p class="lp-suggest"><span class="lp-label">"A note from the reviewer"</span>{n}</p> })}
    }
}

// -----------------------------------------------------------------------------
// 8. One chooser for the whole site
// -----------------------------------------------------------------------------

/// Which listing's chooser is open, and the control that opened it (the focus goes back to it on close). One of these is shared
/// by every page: a Play button anywhere calls `open`, and the host below draws the dialog once.
#[derive(Clone, Copy)]
pub struct PlayChooserSlot {
    sim: RwSignal<Option<String>>,
    opener: StoredValue<Option<web_sys::HtmlElement>, LocalStorage>,
}

impl PlayChooserSlot {
    pub fn open(&self, sim_id: &str) {
        use wasm_bindgen::JsCast;
        let active = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.active_element())
            .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
        self.opener.set_value(active);
        self.sim.set(Some(sim_id.to_string()));
    }

    pub fn is_open(&self) -> bool {
        self.sim.get().is_some()
    }

    fn close(&self) {
        self.sim.set(None);
        if let Some(opener) = self.opener.try_update_value(|o| o.take()).flatten() {
            let _ = opener.focus();
        }
    }
}

pub fn provide_play_chooser() {
    provide_context(PlayChooserSlot { sim: RwSignal::new(None), opener: StoredValue::new_local(None) });
}

/// Draws the chooser of the listing the slot holds. Mounted once, beside the router.
#[component]
pub fn PlayChooserHost() -> impl IntoView {
    let slot = expect_context::<PlayChooserSlot>();
    view! {
        {move || slot.sim.get().map(|id| view! { <PlayChooser sim_id=id on_close=Callback::new(move |_| slot.close()) /> })}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_adult_or_unrated_listing_is_read_again_before_its_signed_image_urls_lapse() {
        let mut l = Listing::default();
        for (rating, signed) in [(Some("adult_18"), true), (Some("adult_21"), true), (None, true), (Some("unrated"), true), (Some("all_ages"), false), (Some("ages_10"), false), (Some("teen_13"), false), (Some("mature_17"), false)] {
            l.rating = rating.map(str::to_string);
            assert_eq!(signed_images(&l), signed, "{rating:?}");
        }
        // the Worker's URLs live at least 15 minutes
        assert!(RESIGN_MS < 15.0 * 60_000.0 && RESIGN_MS >= 60_000.0);
        let src = include_str!("play_chooser.rs");
        assert!(src.contains("if signed_images(&listing) && (shown_again || now - last_listing >= RESIGN_MS) {"));
        assert!(src.contains("if !matches!(answer, Load::Failed(_)) && answer != Load::Ready(listing.clone()) {"));
    }

    #[test]
    fn an_adult_listing_held_back_by_the_setting_gets_its_notice_and_any_other_403_reads_as_missing() {
        let forbidden = |body: &str| Err(ApiError::Server { status: 403, message: body.to_string() });
        assert_eq!(load_of(forbidden(r#"{"code":"adult_content_off","signed_in":true}"#)), Load::AdultOff { signed_in: true });
        assert_eq!(load_of(forbidden(r#"{"code":"adult_content_off","signed_in":false}"#)), Load::AdultOff { signed_in: false });
        assert_eq!(load_of(forbidden(r#"{"error":"For adults 21 and over.","code":"age_21_required","signed_in":true}"#)), Load::Age21Required("For adults 21 and over.".to_string()));
        assert_eq!(load_of(forbidden(r#"{"error":"Forbidden"}"#)), Load::Missing);
        assert_eq!(load_of(forbidden("Forbidden")), Load::Missing);
        assert_eq!(load_of(Err(ApiError::NotFound)), Load::Missing);
        assert!(matches!(load_of(Err(ApiError::Server { status: 500, message: String::new() })), Load::Failed(_)));
    }

    #[test]
    fn only_a_409_from_the_manifest_asks_the_author_to_republish() {
        assert!(needs_republish(&ApiError::Server { status: 409, message: "This simulation was published as a .pak".into() }));
        for e in [ApiError::Server { status: 500, message: String::new() }, ApiError::Server { status: 403, message: String::new() }, ApiError::NotFound, ApiError::Unauthorized, ApiError::Network("x".into())] {
            assert!(!needs_republish(&e), "{e:?}");
        }
        assert_eq!(REPUBLISH_NOTE, "Republish once to make your listing playable again.");
        let src = include_str!("play_chooser.rs");
        assert!(src.contains("listing.format.as_deref() != Some(\"echk\")") && src.contains("!is_author(&listing)"), "asked only for an author's .echk listing");
        assert!(src.contains("(author && republish.get())"), "shown only to the author");
    }

    fn facts(status: Status, live: Live) -> Facts {
        Facts { status, author: true, person_review: false, live, viewer: Viewer::Desktop }
    }

    fn hosted(joinable: bool) -> Live {
        Live::Hosted { players: 2, max_players: 8, joinable, full: false, servers: Vec::new() }
    }

    fn listing(listable: Option<bool>, status: &str) -> Listing {
        Listing { listable, moderation: Some(ReviewView { status: status.to_string(), ..Default::default() }), ..Default::default() }
    }

    #[test]
    fn a_listing_in_the_gallery_is_listed_and_any_other_takes_its_status_from_the_review() {
        assert_eq!(status_of(&Listing::default()), Status::Listed, "a gallery record says nothing of review");
        for (status, want) in [
            ("listed", Status::Listed),
            ("approved_private", Status::Private),
            ("in_review", Status::InReview),
            ("changes_requested", Status::ChangesRequested),
            ("not_listed", Status::NotListed),
            ("appeal_in_review", Status::AppealInReview),
            ("unreviewed", Status::Unreviewed),
            ("something new", Status::Unreviewed),
        ] {
            assert_eq!(status_of(&listing(Some(false), status)), want, "{status}");
        }
        assert_eq!(status_of(&Listing { listable: Some(false), ..Default::default() }), Status::Unreviewed, "no review view at all");
    }

    #[test]
    fn the_play_area_has_a_clear_state_for_every_review_status_and_every_hosting_state() {
        assert_eq!(panel_for(&facts(Status::Listed, hosted(true))), Panel::Join { players: 2, max_players: 8 });
        assert_eq!(panel_for(&facts(Status::Listed, hosted(false))), Panel::SignInToJoin { players: 2, max_players: 8 });
        assert_eq!(panel_for(&facts(Status::Listed, Live::Offline)), Panel::Solo { offline: true, private: false });
        assert_eq!(panel_for(&facts(Status::Listed, Live::Unknown)), Panel::Solo { offline: false, private: false });
        assert_eq!(panel_for(&facts(Status::Private, Live::Offline)), Panel::Solo { offline: true, private: true });
        assert_eq!(panel_for(&facts(Status::Private, hosted(true))), Panel::Join { players: 2, max_players: 8 });
        assert_eq!(panel_for(&facts(Status::InReview, Live::Offline)), Panel::InReview { person: false });
        let held = Facts { person_review: true, ..facts(Status::InReview, Live::Unknown) };
        assert_eq!(panel_for(&held), Panel::InReview { person: true });
        assert_eq!(panel_for(&facts(Status::ChangesRequested, Live::Unknown)), Panel::ChangesRequested);
        assert_eq!(panel_for(&facts(Status::NotListed, Live::Unknown)), Panel::NotListed);
        assert_eq!(panel_for(&facts(Status::AppealInReview, Live::Unknown)), Panel::AppealInReview);
        assert_eq!(panel_for(&facts(Status::Unreviewed, Live::Unknown)), Panel::Unreviewed);
    }

    #[test]
    fn no_play_button_is_offered_for_a_listing_that_cannot_be_played() {
        // Only these two panels carry a Play button in the page; none of the review panels is one of them.
        let playable = |p: &Panel| matches!(p, Panel::Join { .. } | Panel::SignInToJoin { .. } | Panel::MacCannotJoin { .. } | Panel::Solo { .. });
        for status in [Status::InReview, Status::ChangesRequested, Status::NotListed, Status::AppealInReview, Status::Unreviewed] {
            for live in [Live::Unknown, Live::Offline, hosted(true), hosted(false)] {
                for person_review in [false, true] {
                    let f = Facts { person_review, ..facts(status, live.clone()) };
                    assert!(!playable(&panel_for(&f)), "{status:?} {live:?}");
                }
            }
        }
        for status in [Status::Listed, Status::Private] {
            assert!(playable(&panel_for(&facts(status, Live::Offline))));
        }
    }

    #[test]
    fn a_computer_that_cannot_join_is_not_offered_a_join_and_a_phone_is_not_offered_play() {
        let mac = Facts { viewer: Viewer::Mac, ..facts(Status::Listed, hosted(true)) };
        assert_eq!(panel_for(&mac), Panel::MacCannotJoin { players: 2, max_players: 8 });
        let mac_offline = Facts { viewer: Viewer::Mac, ..facts(Status::Listed, Live::Offline) };
        assert_eq!(panel_for(&mac_offline), Panel::Solo { offline: true, private: false }, "a Mac plays alone as anyone does");
        let phone = Facts { viewer: Viewer::Phone, ..facts(Status::Listed, hosted(true)) };
        assert_eq!(panel_for(&phone), Panel::NeedsComputer);
        // A phone still sees the review states.
        let phone_review = Facts { viewer: Viewer::Phone, ..facts(Status::InReview, Live::Unknown) };
        assert_eq!(panel_for(&phone_review), Panel::InReview { person: false });
        assert_eq!(Viewer::from_user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/130"), Viewer::Desktop);
        assert_eq!(Viewer::from_user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Safari/605"), Viewer::Mac);
        assert_eq!(Viewer::from_user_agent("Mozilla/5.0 (X11; Linux x86_64) Firefox/130"), Viewer::Desktop);
        assert_eq!(Viewer::from_user_agent("Mozilla/5.0 (Linux; Android 14) Chrome/130 Mobile"), Viewer::Phone);
        assert_eq!(Viewer::from_user_agent("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)"), Viewer::Phone);
    }

    #[test]
    fn the_words_for_an_open_review_are_the_ones_the_owner_asked_for_and_say_no_reason() {
        let (title, text) = panel_text(&Panel::InReview { person: false }, true, "We are reviewing your listing. This usually takes under a minute.");
        assert_eq!(title, "In review");
        assert_eq!(text, "Your Space is being reviewed. You can play it once it's approved, usually within a minute.");
        let (title, text) = panel_text(&Panel::InReview { person: true }, true, "A person is reviewing your listing.");
        assert_eq!(title, "Held for a person to review");
        assert_eq!(text, "A person is reviewing your space. You will hear from us when it is decided. You can play it once it is approved.");
        let (_, other) = panel_text(&Panel::InReview { person: false }, false, "");
        assert!(other.starts_with("This Space is being reviewed."), "{other}");
        // No wording of an open review is the Worker's headline, which can say more than the page should repeat.
        for person in [false, true] {
            let (_, t) = panel_text(&Panel::InReview { person }, true, "SECRET REASON");
            assert!(!t.contains("SECRET"));
        }
    }

    #[test]
    fn a_decision_is_worded_by_the_worker_and_never_by_the_page() {
        let (_, text) = panel_text(&Panel::ChangesRequested, true, "Your Universe needs a few changes before it can be listed.");
        assert_eq!(text, "Your Universe needs a few changes before it can be listed.");
        let (_, text) = panel_text(&Panel::NotListed, true, "Your Universe was not listed in the Gallery.");
        assert_eq!(text, "Your Universe was not listed in the Gallery.");
        // Without a headline there is a plain sentence, still no reason.
        let (title, text) = panel_text(&Panel::NotListed, true, "");
        assert_eq!((title.as_str(), text.as_str()), ("Not listed", "Your Space was not listed in the Gallery."));
    }

    #[test]
    fn playing_alone_says_nobody_hosts_it_only_when_the_registry_said_so() {
        let (_, offline) = panel_text(&Panel::Solo { offline: true, private: false }, true, "");
        assert!(offline.contains("Nobody is hosting a shared session right now"), "{offline}");
        let (_, unknown) = panel_text(&Panel::Solo { offline: false, private: false }, true, "");
        assert!(!unknown.contains("Nobody is hosting"), "{unknown}");
        let (_, private) = panel_text(&Panel::Solo { offline: false, private: true }, true, "");
        assert!(private.contains("only you can play it"));
        let (title, joined) = panel_text(&Panel::Join { players: 1, max_players: 1 }, true, "");
        assert_eq!(title, "Live now");
        assert!(joined.starts_with("1 of 1 player are in"), "{joined}");
    }

    #[test]
    fn the_chips_say_what_the_registry_said() {
        assert_eq!(live_chip(&Live::Unknown), None);
        assert_eq!(live_chip(&Live::Offline), Some(("lp-chip plain", "Offline: nobody is hosting right now".to_string())));
        assert_eq!(live_chip(&hosted(true)), Some(("lp-chip live", "Live: 2 of 8 players".to_string())));
        assert_eq!(live_chip(&hosted(false)), Some(("lp-chip live", "Live: 2 of 8 players".to_string())), "a signed-out visitor sees it is live");
    }

    #[test]
    fn only_the_players_own_links_reach_the_browser() {
        let pin = "ab".repeat(32);
        let join = format!("eustress-player://join/192.168.1.20:7777?key={}&pin={pin}", "0123456789abcdef".repeat(2));
        assert!(is_player_link(&join));
        assert!(is_player_link("eustress-player://play/aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa"));
        for bad in ["javascript:alert(1)", "https://example.com/", "eustress://join/1.2.3.4:5?key=a&pin=b", "eustress-player://play/", "eustress-player://play/a b", "eustress-player://play/../x", "eustress-player://exec/calc", ""] {
            assert!(!is_player_link(bad), "{bad}");
        }
    }

    fn host_link() -> String {
        format!("eustress-player://join/192.168.1.20:7777?key={}&pin={}", "0123456789abcdef".repeat(2), "ab".repeat(32))
    }

    #[test]
    fn a_good_ticket_is_appended_after_the_hosts_query_with_the_api_name() {
        let join = host_link();
        let linked = with_ticket(&join, "eyJhbGciOi.JIUzI1_NiIsInR5-cCI6", "staging");
        assert_eq!(linked, format!("{join}&ticket=eyJhbGciOi.JIUzI1_NiIsInR5-cCI6&api=staging"));
        assert!(linked.starts_with(&join), "the host's own query is kept, in order");
        assert_eq!(with_ticket(&join, "abc", "production"), format!("{join}&ticket=abc&api=production"));
        // A link with no query starts one; a ticket of exactly 2,048 characters is the longest allowed.
        assert_eq!(with_ticket("eustress-player://join/h", "abc", "staging"), "eustress-player://join/h?ticket=abc&api=staging");
        let longest = "a".repeat(2_048);
        assert_eq!(with_ticket(&join, &longest, "staging"), format!("{join}&ticket={longest}&api=staging"));
    }

    #[test]
    fn a_ticket_that_is_empty_too_long_or_has_a_stray_character_is_left_out_and_the_link_keeps_only_the_api_name() {
        let join = host_link();
        let only_api = format!("{join}&api=production");
        assert_eq!(with_ticket(&join, "", "production"), only_api);
        assert_eq!(with_ticket(&join, &"a".repeat(2_049), "production"), only_api);
        for bad in ["a b", "a+b", "a/b", "a&b=c", "a=b", "a%20b", "a#b", "a?b", "a\"b", "a'b", "a\nb", "a\u{e9}b", "a:b", "a<b", " ", "ticket;x"] {
            let linked = with_ticket(&join, bad, "production");
            assert_eq!(linked, only_api, "{bad:?}");
            assert!(!linked.contains("ticket="), "{bad:?}");
        }
        assert_eq!(with_ticket("eustress-player://join/h", "", "staging"), "eustress-player://join/h?api=staging");
        assert!(is_ticket("a.b_c-D9") && !is_ticket("") && !is_ticket("a b"));
    }

    #[test]
    fn a_solo_link_gets_the_api_name_and_never_a_ticket() {
        let solo = "eustress-player://play/aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa";
        assert_eq!(launch_link(solo, None, "production"), solo, "production solo links carry no api until a Player release reads it");
        // Staging is tested with a Player that reads `api` (build 32, P1), so its solo links name the staging API.
        let staged = launch_link(solo, None, "staging");
        assert_eq!(staged, format!("{solo}?api=staging"));
        assert!(is_player_link(&staged), "the staging solo link is one the page may hand to the browser");
        assert_eq!(launch_link(&host_link(), None, "staging"), format!("{}&api=staging", host_link()));
        assert_eq!(launch_link(&host_link(), Some("tk.1_2-3"), "staging"), format!("{}&ticket=tk.1_2-3&api=staging", host_link()));
        // A ticket that would make the link something the Player's grammar does not allow is dropped, never the link.
        let long_worker_link = format!("eustress-player://join/h?pin={}", "a".repeat(500));
        assert_eq!(launch_link(&long_worker_link, Some(&"b".repeat(2_048)), "staging"), format!("{long_worker_link}&api=staging"));
        assert_eq!(launch_link(&host_link(), Some("a b"), "staging"), format!("{}&api=staging", host_link()));
    }

    #[test]
    fn the_site_host_names_the_api_the_player_should_use() {
        assert_eq!(api_name_for_host("staging.eustress-cbb.pages.dev"), "staging");
        for production in ["eustress.dev", "www.eustress.dev", "eustress-cbb.pages.dev", "abc123.eustress-cbb.pages.dev", "x.staging.eustress-cbb.pages.dev", "staging.eustress-cbb.pages.dev.evil.example", "Staging.eustress-cbb.pages.dev", "localhost", "127.0.0.1", ""] {
            assert_eq!(api_name_for_host(production), "production", "{production}");
        }
        // The page and the Player never name different APIs for the staging site.
        assert_eq!(crate::state::api_base_for_host("staging.eustress-cbb.pages.dev"), crate::state::STAGING_API);
        assert_eq!(crate::state::api_base_for_host("eustress.dev"), crate::state::PRODUCTION_API);
    }

    #[test]
    fn a_solo_link_may_end_in_one_of_two_api_names_and_nothing_else() {
        let id = "aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa";
        for good in [format!("eustress-player://play/{id}"), format!("eustress-player://play/{id}?api=production"), format!("eustress-player://play/{id}?api=staging")] {
            assert!(is_player_link(&good), "{good}");
        }
        for bad in ["?api=", "?api=dev", "?api=Production", "?api=productionx", "?api=production&x=1", "?api=staging&api=staging", "?x=1", "?", "?ticket=abc&api=staging", "?api=staging#x", "?api=staging ", " ?api=staging", "&api=staging", "?api=production?api=staging"] {
            let link = format!("eustress-player://play/{id}{bad}");
            assert!(!is_player_link(&link), "{link}");
        }
        assert!(!is_player_link("eustress-player://play/?api=staging"), "an id is required");
        assert!(!is_player_link("eustress-player://play/a b?api=staging"));
        assert!(!is_player_link("eustress-player://play/../x?api=staging"));
    }

    #[test]
    fn a_join_link_with_the_appended_parameters_reaches_the_browser_and_nothing_else_does() {
        let join = host_link();
        assert!(is_player_link(&format!("{join}&ticket=eyJ.a_b-c&api=staging")));
        assert!(is_player_link(&format!("{join}&api=production")));
        assert!(is_player_link(&join));
        for bad in [
            "javascript:alert(1)".to_string(),
            "https://example.com/".to_string(),
            "eustress://join/1.2.3.4:5?key=a&pin=b".to_string(),
            "eustress-player://exec/calc".to_string(),
            format!("{join}&ticket=a b&api=staging"),
            format!("{join}&ticket=a\"b"),
            format!("{join}&ticket={}", "a".repeat(2_600)),
            format!("{join}x/../ y"),
        ] {
            assert!(!is_player_link(&bad), "{bad:.80}");
        }
    }

    #[test]
    fn the_workers_own_link_is_still_held_to_512_characters_and_to_the_bare_forms() {
        let pin = "ab".repeat(32);
        let long = format!("eustress-player://join/h?key={}&pin={pin}", "a".repeat(512));
        assert!(long.len() > 512);
        assert!(is_player_link(&long), "the final link may be up to 512 + 2,048 + 32");
        assert!(chosen_link(&answer("live", Some(&long), None), Kind::Join).is_err(), "the Worker's join link is capped before anything is appended");
        let solo = "eustress-player://play/aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa";
        let with_query = format!("{solo}?api=staging");
        assert!(chosen_link(&answer("solo", Some(&with_query), Some(&with_query)), Kind::Solo).is_err(), "the Worker sends no query; the chooser adds it");
        assert_eq!(chosen_link(&answer("solo", Some(solo), Some(solo)), Kind::Solo).as_deref(), Ok(solo));
    }

    #[test]
    fn a_ticket_is_minted_only_for_a_signed_in_viewer_joining_a_live_link_with_a_pin() {
        let join = host_link();
        let pin = "ab".repeat(32);
        assert_eq!(ticket_audience(Kind::Join, true, &join), Some(pin.as_str()));
        assert_eq!(ticket_audience(Kind::Join, false, &join), None, "signed out: no ticket");
        assert_eq!(ticket_audience(Kind::Solo, true, &join), None, "playing alone: no ticket, ever");
        assert_eq!(ticket_audience(Kind::Join, true, "eustress-player://play/aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa"), None);
        assert_eq!(ticket_audience(Kind::Join, true, "eustress-player://join/h:1?key=a"), None, "no pin");
        assert_eq!(pin_of(&format!("eustress-player://join/h:1?pin={}", "a".repeat(63))), None);
        assert_eq!(pin_of(&format!("eustress-player://join/h:1?pin={}", "a".repeat(65))), None);
        assert_eq!(pin_of(&format!("eustress-player://join/h:1?pin={}", "g".repeat(64))), None, "not hex");
        let both = "0f".repeat(32);
        assert_eq!(pin_of(&format!("eustress-player://join/h:1?key=a&pin={both}&x=1")), Some(both.as_str()));
        assert_eq!(pin_of("eustress-player://join/h:1"), None);
    }

    // The code below is this file's own source (without its tests), read back so the ticket rules stay true when the code changes.
    fn code() -> &'static str {
        include_str!("play_chooser.rs").split("#[cfg(test)]").next().unwrap()
    }

    fn between<'a>(source: &'a str, from: &str, to: &str) -> &'a str {
        let start = source.find(from).unwrap_or_else(|| panic!("no {from}"));
        let rest = &source[start..];
        &rest[..rest.find(to).unwrap_or_else(|| panic!("no {to}"))]
    }

    fn without_comment(line: &str) -> &str {
        line.split("//").next().unwrap()
    }

    #[test]
    fn the_ticketed_link_exists_in_one_variable_that_only_open_player_receives() {
        let play = between(code(), "let play = Callback::new(", "// Escape closes.");
        let lines: Vec<&str> = play.lines().map(without_comment).collect();
        let holders: Vec<&str> = lines.iter().copied().filter(|l| l.contains("ticketed_link")).collect();
        assert_eq!(holders.len(), 2, "{holders:?}");
        assert!(holders[0].trim_start().starts_with("let ticketed_link = launch_link("), "{}", holders[0]);
        assert_eq!(holders[1].trim(), "open_player(&ticketed_link);");
        // What the page keeps is built without a ticket, and every Handoff in the callback is given only that.
        assert!(lines.iter().any(|l| l.trim() == "let shown = launch_link(&link, None, api);"));
        let handoffs: Vec<&str> = lines.iter().copied().filter(|l| l.contains("Handoff::Opening") || l.contains("Handoff::NotOpened") || l.contains("watch_launch")).collect();
        assert!(handoffs.len() >= 2, "{handoffs:?}");
        for line in handoffs {
            assert!(line.contains("shown") && !line.contains("ticket"), "{line}");
        }
        // Nothing that stores, shows, logs or words an error is on a line with the ticket.
        for line in lines.iter().filter(|l| l.contains("ticket")) {
            for forbidden in ["handoff", "Handoff", "log::", "console", "format!", "println", "dbg!", "set(", "set_value", "Storage", "storage", "history", "failure_sentence", "to_string", "view!", "href", "RwSignal", "StoredValue", "Failed"] {
                assert!(!line.contains(forbidden), "{forbidden}: {line}");
            }
        }
    }

    #[test]
    fn no_log_console_storage_or_page_text_in_the_code_mentions_a_ticket() {
        for line in code().lines().filter(|l| !l.trim_start().starts_with("//")) {
            let line = without_comment(line);
            if !line.to_ascii_lowercase().contains("ticket") {
                continue;
            }
            for forbidden in ["log::", "console", "println", "eprintln", "dbg!", "tracing", "alert", "set_item", "Storage", "view!", "href=", "class=", "history", "panic!", "expect("] {
                assert!(!line.contains(forbidden), "{forbidden}: {line}");
            }
        }
        // The one function that takes a ticket from the network drops a failure; it does not word one.
        let mint = between(code(), "async fn mint_ticket", "\n}");
        for forbidden in ["format!", "to_string", "Err(", "{e", "{:?}", "unwrap", "expect", "log::", "console", "Debug"] {
            assert!(!mint.contains(forbidden), "{forbidden}");
        }
        assert!(mint.contains("answer.ok()"));
        assert!(mint.contains("TimeoutFuture::new(TICKET_BUDGET_MS)"));
        let answer_type: String = between(code(), "/// `POST /api/identity/ticket`", "struct TicketAnswer").lines().filter(|l| !l.trim_start().starts_with("//")).collect();
        assert!(answer_type.contains("#[derive(Deserialize)]"));
        assert!(!answer_type.contains("Debug") && !answer_type.contains("Clone") && !answer_type.contains("Serialize,"), "the answer type must not print or copy its ticket");
        assert_eq!(code().matches("const TICKET_BUDGET_MS: u32 = 2_500;").count(), 1);
        assert_eq!(code().matches("\"/api/identity/ticket\"").count(), 1);
    }

    #[test]
    fn the_ticketed_link_is_checked_against_the_final_grammar_and_the_workers_link_against_its_own() {
        let launch = between(code(), "pub fn launch_link(", "\n}");
        assert!(launch.contains("is_launch_link(&full)"));
        let chosen = between(code(), "pub fn chosen_link(", "\n}");
        assert!(chosen.contains("is_worker_link(&l)") && !chosen.contains("is_player_link"));
    }

    fn answer(status: &str, link: Option<&str>, solo: Option<&str>) -> PlayResponse {
        PlayResponse { status: status.to_string(), launch: Some(Launch { link: link.map(str::to_string), solo_link: solo.map(str::to_string) }) }
    }

    #[test]
    fn join_opens_the_hosts_link_only_when_the_worker_says_live_and_solo_opens_the_world() {
        let pin = "ab".repeat(32);
        let join = format!("eustress-player://join/192.168.1.20:7777?key={}&pin={pin}", "0123456789abcdef".repeat(2));
        let solo = "eustress-player://play/aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa";
        let live = answer("live", Some(&join), Some(solo));
        assert_eq!(chosen_link(&live, Kind::Join).as_deref(), Ok(join.as_str()));
        assert_eq!(chosen_link(&live, Kind::Solo).as_deref(), Ok(solo), "alone, even beside a live session");
        // The host left between the page and the click: the Worker answers solo, and Join says so instead of playing alone.
        let gone = answer("solo", Some(solo), Some(solo));
        assert!(chosen_link(&gone, Kind::Join).unwrap_err().contains("just ended"));
        assert_eq!(chosen_link(&gone, Kind::Solo).as_deref(), Ok(solo));
        // A link that is not the Player's is never opened, whoever sent it.
        let bad = answer("live", Some("javascript:alert(1)"), Some("https://example.com/"));
        assert!(chosen_link(&bad, Kind::Join).is_err());
        assert!(chosen_link(&bad, Kind::Solo).is_err());
        // An older Worker sends no solo_link: the play link in `link` serves.
        let old = answer("solo", Some(solo), None);
        assert_eq!(chosen_link(&old, Kind::Solo).as_deref(), Ok(solo));
        assert!(chosen_link(&answer("solo", None, None), Kind::Solo).is_err());
    }

    #[test]
    fn the_page_asks_again_when_something_can_change_and_never_when_only_the_author_can_change_it() {
        assert_eq!(poll_interval_ms(Status::InReview, false), Some(6_000.0));
        assert_eq!(poll_interval_ms(Status::InReview, true), Some(30_000.0), "a person's review is slower");
        assert_eq!(poll_interval_ms(Status::AppealInReview, false), Some(6_000.0));
        assert_eq!(poll_interval_ms(Status::Listed, false), Some(30_000.0));
        for decided in [Status::ChangesRequested, Status::NotListed, Status::Unreviewed] {
            assert_eq!(poll_interval_ms(decided, false), None, "{decided:?}");
        }
    }
}
