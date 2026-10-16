//! # Hosting a session from Studio
//!
//! **Start Server** (F9, Network > Start Local Server, or Start in the Test
//! tab's Server group) hosts the Space being played so Players can join it:
//!
//! 1. Save edits not yet saved, when editing, so players get what the host
//!    sees.
//! 2. Bake the Space and the Universe's shared `assets/` into `.echk` chunks
//!    off the main thread: the same export publishing uploads
//!    ([`crate::space::echk_export`]).
//! 3. Listen for WebTransport sessions ([`eustress_networking::native`]):
//!    on this machine only by default, on the local network when asked.
//! 4. Enter Play with a character, if not already playing.
//! 5. Show the join link, and leave it where a Player on this machine finds
//!    it (`<workspace>/.eustress/hosts/<port>.link`).
//!
//! **Stop Server**, or returning to Edit, ends the session. Pausing does not.
//!
//! The host is authoritative: its scripts and physics run here, and players
//! see the host's world plus every avatar in real time.
//!
//! ## Settings
//!
//! | Variable | Default | Meaning |
//! |---|---|---|
//! | `EUSTRESS_HOST_PORT` | 7777 | Port to listen on (0 picks any free port) |
//! | `EUSTRESS_HOST_LAN` | off | `1` also accepts players on the local network |
//! | `EUSTRESS_HOST_MAX_PLAYERS` | unset | A developer override of the hosted Space's `Players.MaxPlayers` (everyone counted); the Output says when it is in use |
//! | `EUSTRESS_HOST_ON_PLAY` | off | `1` hosts whenever Play starts; with `eustress-headless` that is a dedicated server |
//! | `EUSTRESS_HOST_PUBLIC` | unset | The address players reach this host at, for the gallery's link (`host` or `host:port`, an IPv4 address or a name): this computer's local network address when players come in through the tunnel's private route, a public name later. A host listens on IPv4 only, so an IPv6 address keeps the session off the gallery |
//!
//! ## The gallery
//!
//! Hosting a published simulation, signed in as the listing's author, with
//! `EUSTRESS_HOST_LAN=1` and `EUSTRESS_HOST_PUBLIC` set, tells the gallery
//! every 30 seconds that the session is live, with its join link, and tells it
//! again when the session ends. The address is only ever the one configured:
//! guessing this computer's own can pick a VPN or tunnel adapter instead.

use std::collections::{BTreeSet, HashMap};
use std::sync::{mpsc, Arc, Mutex};

use bevy::ecs::message::{MessageCursor, Messages};
use bevy::prelude::*;

use eustress_echk::{Destination, WorldManifest};
use eustress_networking::native::{self, HostOptions};
use eustress_networking::session::ADMISSION_UNCONFIRMED;
use eustress_networking::wire::PeerId;
use eustress_networking::{
    begin_host, AdmissionVerdict, Admitted, EndSession, HostConfig, HostSession, JoinLink, NetNotice, NetPlugin,
};

use crate::keybindings::Action;
use crate::notifications::NotificationManager;
use crate::play_mode::{PlayModeState, PlayModeType, StartPlayEvent};
use crate::ui::MenuActionEvent;
use crate::ui::slint_ui::OutputConsole;

/// The baked world, each streamed core's record key by stored id, when
/// hosting begins in a Play session already running, what players load, and
/// the lines the export's screen asks the host to read (SoulService's notice,
/// key warnings).
type ExportResult = Result<
    (
        WorldManifest,
        HashMap<String, Arc<Vec<u8>>>,
        HashMap<u64, String>,
        Option<eustress_networking::repl::WorldLayout>,
        Vec<String>,
    ),
    String,
>;

struct PendingExport {
    /// How the host will listen, read once before the bake: its `lan` chose
    /// the export's destination, and the same value binds the listener.
    options: HostOptions,
    rx: Mutex<mpsc::Receiver<ExportResult>>,
    space: String,
    /// The Space folder the world's record keys are relative to.
    space_root: std::path::PathBuf,
    host_name: String,
    spawn: Vec3,
    start_play: bool,
    /// The gallery listing this Universe is published as, if any.
    sim_id: Option<String>,
    /// The hosted Space's `Players.MaxPlayers`.
    max_players: eustress_common::play_session::SpaceMaxPlayers,
}

struct ActiveHost {
    join_key: String,
    lan: bool,
    port: u16,
    /// The certificate pin, once listening.
    pin: Option<[u8; 32]>,
    saw_playing: bool,
}

/// Where hosting is: asked for, baking, or running.
#[derive(Resource, Default)]
pub struct HostRequest {
    wanted: bool,
    pending: Option<PendingExport>,
    active: Option<ActiveHost>,
}

impl HostRequest {
    pub fn is_hosting(&self) -> bool {
        self.active.is_some()
    }
}

/// Adds hosting. Registered in `app_core`, so the headless engine can host
/// too (see `EUSTRESS_HOST_ON_PLAY`).
pub struct MultiplayerPlugin;

impl Plugin for MultiplayerPlugin {
    fn build(&self, app: &mut App) {
        // The API in use, resolved at startup, so a test API
        // (`EUSTRESS_API_URL`) is announced before any call goes to it.
        let _ = eustress_common::api_base::api_base();
        app.add_plugins((NetPlugin, crate::net_replicate::ReplicationPlugin))
            .init_resource::<HostRequest>()
            .init_resource::<GalleryBeat>()
            .init_resource::<GalleryAdmissions>()
            .init_resource::<OpenSpaceMaxPlayers>()
            .add_systems(Update, track_space_max_players)
            .init_resource::<MyListings>()
            .add_message::<RefreshMyListings>()
            .add_systems(Update, refresh_my_listings)
            .init_resource::<MyChangeLogs>()
            .add_message::<RefreshMyChangeLogs>()
            .add_message::<DownloadChangeLog>()
            .add_systems(Update, refresh_my_change_logs)
            .add_systems(
                Update,
                (
                    read_server_actions,
                    host_on_play,
                    start_host_export,
                    poll_host_export,
                    stop_when_play_ends,
                    report_net_notices,
                    report_movement_check,
                    track_host_character,
                    refresh_host_file,
                    gallery_heartbeat,
                    admit_gallery_joins,
                )
                    .chain(),
            )
            // After Update, so it sees an exit asked for anywhere this frame.
            .add_systems(Last, stop_listing_on_exit);
    }
}

/// Seconds between the beats that keep a session live in the gallery; it
/// shows the session offline 90 s after the last one.
const GALLERY_BEAT_SECS: f64 = 30.0;

/// What the gallery was told about this host's session.
#[derive(Resource, Default)]
struct GalleryBeat {
    next_at: f64,
    /// The listing, and the link it shows.
    told: Option<(String, String)>,
    /// Heartbeat answers in the order they came, each with the number its
    /// POST went out with, written by its thread; and why none went out
    /// (number 0), written here.
    answers: Arc<Mutex<Vec<(u64, BeatOutcome)>>>,
    /// The number the last heartbeat POST went out with.
    sent: u64,
    /// This hosting session's heartbeats not yet answered.
    unanswered: BTreeSet<u64>,
    /// The outcome last shown, so a repeated one is shown once.
    reported: Option<BeatOutcome>,
    /// The gallery may have listed this hosting session (a heartbeat answered
    /// ok, or its answer never came): its joins wait for the gallery's
    /// admission for good.
    maybe_listed: bool,
}

/// What became of a heartbeat.
#[derive(Clone, Debug, PartialEq)]
enum BeatOutcome {
    /// The gallery shows the session live.
    Live,
    /// The request went out but no answer came back (`why`), so the gallery
    /// may show it.
    Unanswered(String),
    /// The request never left this machine (`why`): nothing was stored.
    Unreached(String),
    /// The gallery refused it (`why`): nothing of the session is stored.
    Refused(String),
    /// No heartbeat went out (`why`: a setting, the sign-in, the listing).
    NotSent(String),
}

impl BeatOutcome {
    /// What a heartbeat's answer means: a refusal (4xx), or a request that
    /// never left, stored nothing; an answer that never came, or a server
    /// error, leaves open whether the gallery stored the link.
    fn of(answer: Result<(), BeatFailure>) -> Self {
        match answer {
            Ok(()) => Self::Live,
            Err(BeatFailure::Unreached(why)) => Self::Unreached(why),
            Err(BeatFailure::Unanswered(why)) => Self::Unanswered(why),
            Err(BeatFailure::Status(status, _)) if status >= 500 => {
                Self::Unanswered(format!("the gallery answered {status}"))
            }
            Err(BeatFailure::Status(status, body)) => Self::Refused(match status {
                // Two different 404s: an API without the live registry, and
                // one without this listing.
                404 if body.contains("Simulation not found") => {
                    "this API has no listing with this Universe's id; publish the Universe to it first (404)".to_string()
                }
                404 => "this API has no live registry yet (404)".to_string(),
                403 => "only the listing's author can show it live; sign in as the author (403)".to_string(),
                _ => format!("the gallery answered {status}: {}", body.chars().take(200).collect::<String>()),
            }),
        }
    }

    /// Whether the gallery certainly stored nothing of this session.
    fn stored_nothing(&self) -> bool {
        matches!(self, Self::Unreached(_) | Self::Refused(_) | Self::NotSent(_))
    }

    /// The line the Output and the log show.
    fn message(&self) -> String {
        match self {
            Self::Live => "The gallery shows this session as live.".to_string(),
            Self::Unanswered(why) => {
                format!("No answer from the gallery ({why}). Joins wait for its admission in case it listed this session.")
            }
            Self::Unreached(why) => format!("Could not reach the gallery ({why}), so it does not list this session."),
            Self::Refused(why) => format!("The gallery did not list this session: {why}."),
            Self::NotSent(why) => format!("The gallery does not list this session: {why}."),
        }
    }
}

/// A heartbeat that lists the link is going out: from now on joins wait for
/// the gallery's admission (`docs/networking/HOSTED_ADMISSION.md`), so none
/// gets in while the gallery stores the link and its answer is on its way.
/// Every real heartbeat does this, since any of them may be the one that
/// lists the link.
fn admission_for_sent_beat(host: &mut HostSession) {
    host.require_admission();
}

/// What a heartbeat's answer means for admission; `number` is the POST it
/// answers (0: none went out). Any answer that may have stored the link (ok,
/// no answer, a server error), whatever its number, marks the session maybe
/// listed, and it keeps admission for good, whatever a later heartbeat hears:
/// the gallery shows a stored link for 90 s after its last heartbeat. A
/// session never maybe listed opens again only when the gallery certainly
/// stored nothing and every heartbeat of this hosting session is answered:
/// an earlier POST's refusal, arriving after a later POST went out, leaves
/// admission on, since the later one may store the link.
fn admission_after_answer(
    host: &mut HostSession,
    maybe_listed: &mut bool,
    unanswered: &mut BTreeSet<u64>,
    number: u64,
    outcome: &BeatOutcome,
) {
    let this_session = unanswered.remove(&number) || number == 0;
    if !outcome.stored_nothing() {
        *maybe_listed = true;
        host.require_admission();
    } else if this_session && !*maybe_listed && unanswered.is_empty() {
        host.release_admission();
    }
}

/// Why a heartbeat request failed.
#[derive(Debug)]
enum BeatFailure {
    /// The gallery answered with this status and body.
    Status(u16, String),
    /// The request never left this machine (no address, no connection).
    Unreached(String),
    /// The request went out and no answer came back.
    Unanswered(String),
}

impl BeatFailure {
    /// A failed request, as ureq reports it.
    fn of(error: ureq::Error) -> Self {
        match error {
            ureq::Error::Status(status, response) => Self::Status(status, response.into_string().unwrap_or_default()),
            ureq::Error::Transport(t) => {
                let why = format!("could not reach the API: {t}");
                match t.kind() {
                    ureq::ErrorKind::Dns
                    | ureq::ErrorKind::ConnectionFailed
                    | ureq::ErrorKind::InvalidUrl
                    | ureq::ErrorKind::UnknownScheme => Self::Unreached(why),
                    _ => Self::Unanswered(why),
                }
            }
        }
    }
}

/// Why this hosting session is not offered to the gallery, if it is not:
/// `None` when it can be listed. `public_host` is `EUSTRESS_HOST_PUBLIC`'s
/// host, when it is set.
fn unlisted_because(published: bool, public_host: Option<&str>, lan: bool) -> Option<String> {
    if !published {
        return Some(format!(
            "this Universe has no listing on {}; publish it there to list sessions (players can still join with the link)",
            eustress_common::api_base::api_base()
        ));
    }
    // Only an address someone chose: guessing this machine's network address
    // can pick a VPN or tunnel adapter's instead.
    let Some(public_host) = public_host else {
        return Some(
            "set EUSTRESS_HOST_PUBLIC to the address players reach (this computer's local network address when they \
             come in through the tunnel) and start Studio again"
                .to_string(),
        );
    };
    // A host listens on IPv4 only (`native::start_host`): a link naming an
    // IPv6 address would list a session no Player can reach.
    if is_ipv6_literal(public_host) {
        return Some(format!(
            "EUSTRESS_HOST_PUBLIC names an IPv6 address ({public_host}), and a host listens on IPv4 only; set it to \
             this computer's IPv4 address and start Studio again"
        ));
    }
    if !lan {
        return Some("this session listens on this computer only; set EUSTRESS_HOST_LAN=1 and start Studio again".to_string());
    }
    None
}

/// Whether a host is an IPv6 address, bracketed or bare.
fn is_ipv6_literal(host: &str) -> bool {
    host.trim_start_matches('[').trim_end_matches(']').parse::<std::net::Ipv6Addr>().is_ok()
}

/// `EUSTRESS_HOST_PUBLIC`: where players on the internet reach this host,
/// as a host and, when a port forward or tunnel maps a different one, a port.
fn public_address() -> Option<(String, Option<u16>)> {
    let raw = std::env::var("EUSTRESS_HOST_PUBLIC").ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(end) = raw.find(']').filter(|_| raw.starts_with('[')) {
        let port = raw[end + 1..].strip_prefix(':').and_then(|p| p.parse().ok());
        return Some((raw[..=end].to_string(), port));
    }
    match raw.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => match port.parse::<u16>() {
            Ok(p) => Some((host.to_string(), Some(p))),
            Err(_) => Some((raw.to_string(), None)),
        },
        _ => Some((raw.to_string(), None)),
    }
}

/// Keep the gallery's live marker for this session current, and clear it
/// when the session ends. Each change in what the gallery shows, or why it
/// shows nothing, goes to the Output once; hosting goes on either way.
fn gallery_heartbeat(
    time: Res<Time>,
    request: Res<HostRequest>,
    mut host: Option<ResMut<HostSession>>,
    auth: Option<Res<crate::auth::AuthState>>,
    mut beat: ResMut<GalleryBeat>,
    mut output: Option<ResMut<OutputConsole>>,
) {
    let finished = beat.answers.lock().map(|mut a| std::mem::take(&mut *a)).unwrap_or_default();
    for (number, outcome) in finished {
        if let Some(host) = host.as_deref_mut() {
            let beat = &mut *beat;
            admission_after_answer(host, &mut beat.maybe_listed, &mut beat.unanswered, number, &outcome);
        }
        if beat.reported.as_ref() != Some(&outcome) {
            let message = outcome.message();
            if outcome == BeatOutcome::Live {
                info!("multiplayer: {message}");
                if let Some(out) = output.as_mut() {
                    out.info(message);
                }
            } else {
                warn!("multiplayer: {message}");
                if let Some(out) = output.as_mut() {
                    out.warn(message);
                }
            }
            beat.reported = Some(outcome);
        }
    }

    let token = auth.as_deref().and_then(|a| a.get_token()).map(str::to_string);
    let mut unlisted: Option<String> = None;
    let live = host.as_deref().zip(request.active.as_ref()).and_then(|(host, active)| {
        active.pin?;
        let address = public_address();
        unlisted = unlisted_because(host.sim_id().is_some(), address.as_ref().map(|(h, _)| h.as_str()), active.lan);
        let sim = host.sim_id().filter(|_| unlisted.is_none())?.to_string();
        let (address, port) = address?;
        let link = JoinLink { host: address, port: port.unwrap_or(active.port), key: Some(active.join_key.clone()), pin: active.pin }
            .to_link();
        Some((sim, link, host.player_count(), host.max_players()))
    });
    if let Some(why) = unlisted {
        let outcome = BeatOutcome::NotSent(why);
        if beat.reported.as_ref() != Some(&outcome) {
            if let Ok(mut answers) = beat.answers.lock() {
                answers.push((0, outcome));
            }
        }
    }
    let told = beat.told.take();
    let now = time.elapsed_secs_f64();
    match live {
        Some((sim, link, players, max_players)) => {
            if let (Some((old_sim, _)), Some(token)) = (&told, &token) {
                if *old_sim != sim {
                    gallery_request(&beat.answers, 0, "DELETE", old_sim, token, None);
                }
            }
            let changed = told.as_ref().map_or(true, |(s, l)| *s != sim || *l != link);
            if changed || now >= beat.next_at {
                beat.next_at = now + GALLERY_BEAT_SECS;
                match &token {
                    Some(token) => {
                        let admitted = match host.as_deref_mut() {
                            Some(host) => {
                                admission_for_sent_beat(host);
                                host.admitted_ids()
                            }
                            None => Vec::new(),
                        };
                        let body = serde_json::json!({
                            "link": link,
                            "players": players,
                            "max_players": max_players,
                            "protocol": eustress_networking::wire::PROTOCOL_VERSION,
                            "admitted": admitted,
                        });
                        beat.sent += 1;
                        let number = beat.sent;
                        beat.unanswered.insert(number);
                        gallery_request(&beat.answers, number, "POST", &sim, token, Some(body));
                    }
                    None => {
                        let outcome = BeatOutcome::NotSent("sign in as the listing's author to show it live".into());
                        if beat.reported.as_ref() != Some(&outcome) {
                            if let Ok(mut answers) = beat.answers.lock() {
                                answers.push((0, outcome));
                            }
                        }
                    }
                }
            }
            beat.told = Some((sim, link));
        }
        None => {
            if let (Some((sim, _)), Some(token)) = (told, token) {
                gallery_request(&beat.answers, 0, "DELETE", &sim, &token, None);
            }
            // Each hosting session reports, and is listed, afresh: answers
            // to the last one's heartbeats can no longer open admission.
            if request.active.is_none() {
                beat.reported = None;
                beat.maybe_listed = false;
                beat.unanswered.clear();
            }
        }
    }
}

/// The live marker of a listing on the API in use.
fn live_url(sim: &str) -> String {
    eustress_common::api_base::api_url(&format!("/api/simulations/{sim}/live"))
}

/// One live-marker request, on a thread of its own. A POST's answer goes to
/// `answers` with its `number`.
fn gallery_request(
    answers: &Arc<Mutex<Vec<(u64, BeatOutcome)>>>,
    number: u64,
    method: &'static str,
    sim: &str,
    token: &str,
    body: Option<serde_json::Value>,
) {
    let url = live_url(sim);
    let auth = format!("Bearer {token}");
    let answered = answers.clone();
    let started = std::thread::Builder::new().name("eustress-gallery-live".into()).spawn(move || {
        let request = ureq::request(method, &url).timeout(std::time::Duration::from_secs(10)).set("Authorization", &auth);
        let result = match body {
            Some(body) => request.send_json(body),
            None => request.call(),
        };
        let answer = BeatOutcome::of(result.map(|_| ()).map_err(BeatFailure::of));
        if method == "POST" {
            if let Ok(mut answered) = answered.lock() {
                answered.push((number, answer));
            }
        } else if answer != BeatOutcome::Live {
            warn!("multiplayer: the gallery may still show the session live: {}", answer.message());
        }
    });
    if let Err(e) = started {
        warn!("multiplayer: could not start the gallery request: {e}");
        // Nothing went out, and nothing else will answer it.
        if method == "POST" {
            if let Ok(mut answers) = answers.lock() {
                answers.push((number, BeatOutcome::Unreached(format!("could not start the request: {e}"))));
            }
        }
    }
}

/// The listing to tell the session ended, and the token to tell it with:
/// only while the gallery may show it.
fn listing_to_stop(beat: &GalleryBeat, token: Option<&str>) -> Option<(String, String)> {
    let (sim, _) = beat.told.as_ref()?;
    Some((sim.clone(), token?.to_string()))
}

/// Quitting while the gallery may show this session: tell it the session
/// ended before the process goes, so the listing does not show it live for
/// the 90 s the gallery waits for a heartbeat. A short wait, on purpose: the
/// window is closing.
fn stop_listing_on_exit(
    mut exits: MessageReader<bevy::app::AppExit>,
    beat: Res<GalleryBeat>,
    auth: Option<Res<crate::auth::AuthState>>,
) {
    if exits.read().next().is_none() {
        return;
    }
    let Some((sim, token)) = listing_to_stop(&beat, auth.as_deref().and_then(|a| a.get_token())) else { return };
    match ureq::delete(&live_url(&sim))
        .timeout(std::time::Duration::from_secs(3))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
    {
        Ok(_) => info!("multiplayer: told the gallery this session ended"),
        Err(e) => warn!("multiplayer: the gallery may show this session live for up to 90 s: {e}"),
    }
}

/// The gallery's answers to admissions, written by their threads.
#[derive(Resource, Default)]
struct GalleryAdmissions(Arc<Mutex<Vec<(PeerId, Result<Admitted, String>)>>>);

/// A player said `Hello` to the session the gallery lists: ask the gallery
/// whether its ticket admits it, once, off the frame, and hand the answer to
/// the session (`docs/networking/HOSTED_ADMISSION.md`). The question goes
/// with the author's token, the heartbeat's own.
fn admit_gallery_joins(
    mut notices: MessageReader<NetNotice>,
    host: Option<Res<HostSession>>,
    auth: Option<Res<crate::auth::AuthState>>,
    answers: Res<GalleryAdmissions>,
    mut verdicts: MessageWriter<AdmissionVerdict>,
) {
    for notice in notices.read() {
        let NetNotice::Admit { peer, ticket } = notice else { continue };
        let token = auth.as_deref().and_then(|a| a.get_token()).map(str::to_string);
        let session = host.as_deref().and_then(|h| Some((h.sim_id()?.to_string(), h.pin()?)));
        let (Some(token), Some((sim, pin))) = (token, session) else {
            warn!("multiplayer: a join waits for the gallery, but this session cannot ask it (signed out, or not listed)");
            verdicts.write(AdmissionVerdict { peer: *peer, outcome: Err(ADMISSION_UNCONFIRMED.to_string()) });
            continue;
        };
        let (peer, ticket, out) = (*peer, ticket.as_ref().map(|t| t.secret().to_string()).unwrap_or_default(), answers.0.clone());
        let audience: String = pin.iter().map(|b| format!("{b:02x}")).collect();
        let started = std::thread::Builder::new().name("eustress-admission".into()).spawn(move || {
            let outcome = ask_gallery(&sim, &token, &ticket, &audience);
            if let Ok(mut o) = out.lock() {
                o.push((peer, outcome));
            }
        });
        if let Err(e) = started {
            warn!("multiplayer: could not ask the gallery about a join: {e}");
            verdicts.write(AdmissionVerdict { peer, outcome: Err(ADMISSION_UNCONFIRMED.to_string()) });
        }
    }
    let done = answers.0.lock().map(|mut a| std::mem::take(&mut *a)).unwrap_or_default();
    for (peer, outcome) in done {
        verdicts.write(AdmissionVerdict { peer, outcome });
    }
}

/// One admission, as the gallery answers it: the ticket's id and whether the
/// player is a guest, or the reason the player is shown. The gallery's own
/// words go to the player, printable and short; anything that is not its
/// answer (it is busy, unreachable, or has no live host for this listing)
/// reads as unconfirmed.
fn ask_gallery(sim: &str, token: &str, ticket: &str, audience: &str) -> Result<Admitted, String> {
    let url = eustress_common::api_base::api_url(&format!("/api/simulations/{sim}/live/admit"));
    let reply = ureq::post(&url)
        .timeout(std::time::Duration::from_secs(10))
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(serde_json::json!({ "ticket": ticket, "audience": audience }));
    match reply {
        Ok(response) => {
            let body: serde_json::Value = response.into_json().map_err(|_| ADMISSION_UNCONFIRMED.to_string())?;
            let ticket_id = body["id"]
                .as_str()
                .filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(str::to_ascii_lowercase)
                .ok_or_else(|| ADMISSION_UNCONFIRMED.to_string())?;
            Ok(Admitted { ticket_id, guest: body["guest"].as_bool().unwrap_or(false) })
        }
        Err(ureq::Error::Status(403, response)) => {
            let body: serde_json::Value = response.into_json().unwrap_or_default();
            match body["code"].as_str() {
                Some("busy") => Err(ADMISSION_UNCONFIRMED.to_string()),
                Some("not_author") => {
                    warn!("multiplayer: the gallery says this account is not the listing's author, so it admits nobody here");
                    Err(ADMISSION_UNCONFIRMED.to_string())
                }
                _ => {
                    let said: String = body["error"].as_str().unwrap_or_default().chars().filter(|c| !c.is_control()).take(200).collect();
                    Err(if said.trim().is_empty() { "The gallery did not admit this join.".to_string() } else { said })
                }
            }
        }
        Err(_) => Err(ADMISSION_UNCONFIRMED.to_string()),
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes"))
        .unwrap_or(false)
}

fn env_number<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

/// How a host will listen, read once per start, before its bake: the port,
/// whether it listens beyond this machine (`EUSTRESS_HOST_LAN`), and a fresh
/// join key. The same value binds the listener and chooses the export's
/// destination ([`export_destination`]).
#[cfg_attr(not(feature = "world-db"), allow(dead_code))]
fn host_options() -> HostOptions {
    HostOptions {
        port: env_number("EUSTRESS_HOST_PORT", native::DEFAULT_PORT),
        lan: env_flag("EUSTRESS_HOST_LAN"),
        join_key: native::new_join_key(),
    }
}

/// Where a host's export goes: off this machine when the host listens beyond
/// loopback, which `native::start_host` does only when `lan` is set.
#[cfg_attr(not(feature = "world-db"), allow(dead_code))]
fn export_destination(options: &HostOptions) -> Destination {
    if options.lan {
        Destination::LanHost
    } else {
        Destination::LoopbackHost
    }
}

#[derive(Clone, Copy)]
enum Say {
    Info,
    Error,
}

fn notify(world: &mut World, say: Say, message: String) {
    match say {
        Say::Error => error!("multiplayer: {message}"),
        Say::Info => info!("multiplayer: {message}"),
    }
    if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
        match say {
            Say::Error => n.error(message),
            Say::Info => n.info(message),
        }
    }
}

/// The link players use, as this host would print it.
fn join_link(active: &ActiveHost, pin: Option<[u8; 32]>) -> JoinLink {
    let host = if active.lan {
        native::local_network_ip().map(|ip| ip.to_string()).unwrap_or_else(|| "127.0.0.1".into())
    } else {
        "127.0.0.1".into()
    };
    JoinLink { host, port: active.port, key: Some(active.join_key.clone()), pin }
}

// ─────────────────────────────────────────────────────────────────────────────
// Systems
// ─────────────────────────────────────────────────────────────────────────────

/// Start Server / Stop Server from the ribbon, the Test menu and F9. Read
/// through an optional cursor: the headless engine has no menu and no
/// `MenuActionEvent`, and this system must not fail there.
fn read_server_actions(
    messages: Option<Res<Messages<MenuActionEvent>>>,
    mut cursor: Local<MessageCursor<MenuActionEvent>>,
    mut request: ResMut<HostRequest>,
    host: Option<Res<HostSession>>,
    mut end: MessageWriter<EndSession>,
    mut notifications: Option<ResMut<NotificationManager>>,
) {
    let Some(messages) = messages else { return };
    for event in cursor.read(&messages) {
        match event.action {
            Action::StartServer => {
                if let (Some(active), Some(host)) = (&request.active, &host) {
                    let link = join_link(active, host.pin()).to_link();
                    info!("multiplayer: already hosting; join with {link}");
                    if let Some(n) = notifications.as_mut() {
                        n.info(format!("Already hosting. Players join with: {link}"));
                    }
                } else if request.wanted || request.pending.is_some() {
                    if let Some(n) = notifications.as_mut() {
                        n.info("The server is already starting.");
                    }
                } else {
                    request.wanted = true;
                }
            }
            Action::StopServer => {
                if host.is_some() {
                    end.write(EndSession { reason: "the host stopped the server".into() });
                } else if request.pending.take().is_some() || std::mem::take(&mut request.wanted) {
                    if let Some(n) = notifications.as_mut() {
                        n.info("Server start cancelled.");
                    }
                } else if let Some(n) = notifications.as_mut() {
                    n.info("No server is running.");
                }
            }
            _ => {}
        }
    }
}

/// `EUSTRESS_HOST_ON_PLAY=1`: host once each time Play starts.
fn host_on_play(
    state: Option<Res<State<PlayModeState>>>,
    host: Option<Res<HostSession>>,
    mut request: ResMut<HostRequest>,
    mut enabled: Local<Option<bool>>,
    mut asked_this_play: Local<bool>,
) {
    let enabled = *enabled.get_or_insert_with(|| env_flag("EUSTRESS_HOST_ON_PLAY"));
    let Some(state) = state else { return };
    if !enabled {
        return;
    }
    match state.get() {
        PlayModeState::Editing => *asked_this_play = false,
        PlayModeState::Playing => {
            if !*asked_this_play && host.is_none() && request.pending.is_none() && request.active.is_none() {
                *asked_this_play = true;
                request.wanted = true;
            }
        }
        PlayModeState::Paused => {}
    }
}

/// Flush unsaved edits, then bake the Space, with its live terrain, on a
/// worker thread.
fn start_host_export(world: &mut World) {
    if !world.resource::<HostRequest>().wanted {
        return;
    }
    world.resource_mut::<HostRequest>().wanted = false;
    match prepare_export(world) {
        Ok(pending) => {
            let message = format!("Preparing to host {}: baking the world for players...", pending.space);
            world.resource_mut::<HostRequest>().pending = Some(pending);
            notify(world, Say::Info, message);
        }
        Err(message) => notify(world, Say::Error, message),
    }
}

/// One of the signed-in author's own listings on the API in use, as the
/// Publish dialog lists them for Publish As (`GET /api/projects`,
/// `docs/architecture/PUBLISH_AS.md`).
#[derive(Debug, Clone, PartialEq)]
pub struct ListingSummary {
    pub id: String,
    pub title: String,
    /// The author-facing state the API gives: `published`, `in_review`,
    /// `not_listed`, `changes_requested`, `approved_private`, `unreviewed`.
    pub status: String,
    /// When it last changed, ISO 8601.
    pub updated: String,
    pub thumbnail_url: Option<String>,
    /// Its tags as stored (`docs/networking/LISTING_TAGS.md`), so the dialog
    /// starts from them.
    pub tags: Vec<String>,
    /// The review chose its tags, because the author gave none
    /// (`tags_source: "judge"`); the dialog shows them as suggestions.
    pub tags_suggested: bool,
    /// Where its cover is, when it has one (the author sees one still in
    /// review too).
    pub cover_url: Option<String>,
}

/// Where the author's listings stand.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum MyListingsState {
    #[default]
    Idle,
    Loading,
    Ready,
    /// Why they could not be listed, as a sentence.
    Error(String),
}

/// The signed-in author's own listings on the API in use, newest first,
/// fetched off the frame when a [`RefreshMyListings`] arrives.
#[derive(Resource, Default)]
pub struct MyListings {
    pub state: MyListingsState,
    pub items: Vec<ListingSummary>,
    /// This Universe's own listing on the API in use, so the dialog can
    /// mark it.
    pub own: Option<String>,
    pending: Option<Mutex<mpsc::Receiver<Result<Vec<ListingSummary>, String>>>>,
}

/// Ask for [`MyListings`] again (the Publish dialog opening).
#[derive(Message, Debug, Clone, Copy, Default)]
pub struct RefreshMyListings;

/// The author's listings from a `GET /api/projects` answer, newest first;
/// an entry with no id is left out.
fn listings_from(body: &serde_json::Value) -> Vec<ListingSummary> {
    let text = |v: &serde_json::Value| v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let mut items: Vec<ListingSummary> = body["projects"]
        .as_array()
        .map(|projects| {
            projects
                .iter()
                .filter_map(|p| {
                    Some(ListingSummary {
                        id: text(&p["id"])?,
                        title: text(&p["name"]).unwrap_or_else(|| "Untitled".to_string()),
                        status: text(&p["status"]).unwrap_or_else(|| "unreviewed".to_string()),
                        updated: text(&p["last_edited"]).or_else(|| text(&p["published_at"])).unwrap_or_default(),
                        thumbnail_url: text(&p["thumbnail_url"]),
                        tags: p["tags"]
                            .as_array()
                            .map(|tags| tags.iter().filter_map(|t| t.as_str().map(str::to_string)).collect())
                            .unwrap_or_default(),
                        // An older record carries no source: its tags are the author's.
                        tags_suggested: p["tags_source"].as_str() == Some("judge"),
                        cover_url: text(&p["cover_url"]),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    items.sort_by(|a, b| b.updated.cmp(&a.updated));
    items
}

/// The Universe holding `space_root`'s own listing on the API in use, from
/// its `sync.toml` (the Universe's, else the Space's).
fn own_listing(space_root: &std::path::Path) -> Option<String> {
    let universe_root = crate::space::universe_root_for_path(space_root).unwrap_or_else(|| space_root.to_path_buf());
    [universe_root.as_path(), space_root].iter().find_map(|root| {
        eustress_common::load_toml_file::<eustress_common::SyncManifest>(&root.join(".eustress").join("sync.toml"))
            .ok()
            .and_then(|s| s.remote.listing_id().map(str::to_string))
    })
}

/// Fetch [`MyListings`] when asked, off the frame, with the signed-in token,
/// and take the answer when it comes.
fn refresh_my_listings(
    mut asks: MessageReader<RefreshMyListings>,
    auth: Option<Res<crate::auth::AuthState>>,
    root: Option<Res<crate::space::SpaceRoot>>,
    mut listings: ResMut<MyListings>,
) {
    let answer = listings.pending.as_ref().and_then(|rx| {
        let rx = rx.lock().ok()?;
        match rx.try_recv() {
            Ok(answer) => Some(answer),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("The request for your listings stopped without an answer.".into())),
        }
    });
    if let Some(answer) = answer {
        listings.pending = None;
        match answer {
            Ok(items) => {
                listings.items = items;
                listings.state = MyListingsState::Ready;
            }
            Err(why) => {
                listings.items.clear();
                listings.state = MyListingsState::Error(why);
            }
        }
    }
    if asks.read().next().is_none() {
        return;
    }
    asks.clear();
    listings.own = root.as_deref().and_then(|r| own_listing(&r.0));
    let Some(token) = auth.as_deref().and_then(|a| a.get_token()).map(str::to_string) else {
        listings.items.clear();
        listings.state = MyListingsState::Error("Sign in to see your listings.".into());
        return;
    };
    let (tx, rx) = mpsc::channel();
    let url = eustress_common::api_base::api_url("/api/projects?limit=100");
    let started = std::thread::Builder::new().name("eustress-my-listings".into()).spawn(move || {
        let answer = ureq::get(&url)
            .timeout(std::time::Duration::from_secs(10))
            .set("Authorization", &format!("Bearer {token}"))
            .call()
            .map_err(|e| format!("Could not load your listings: {e}"))
            .and_then(|reply| {
                reply.into_json::<serde_json::Value>().map_err(|e| format!("Could not read your listings: {e}"))
            })
            .map(|body| listings_from(&body));
        let _ = tx.send(answer);
    });
    match started {
        Ok(_) => {
            listings.pending = Some(Mutex::new(rx));
            listings.state = MyListingsState::Loading;
        }
        Err(e) => listings.state = MyListingsState::Error(format!("Could not load your listings: {e}")),
    }
}

/// One stored change log of a listing, as `GET /api/simulations/{id}/changelogs`
/// lists them (`docs/architecture/PUBLISH_CHANGELOG.md`).
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeLogSummary {
    /// The log's id within its listing.
    pub id: String,
    /// The listing's version the log was written for.
    pub version: u32,
    pub publish_hash: String,
    /// When it was stored, ISO 8601 UTC.
    pub created_at: String,
    pub bytes: u64,
}

/// The stored change logs of one of the author's listings, newest first,
/// fetched off the frame when a [`RefreshMyChangeLogs`] arrives, and how
/// each [`DownloadChangeLog`] went.
#[derive(Resource, Default)]
pub struct MyChangeLogs {
    pub state: MyListingsState,
    /// The listing `items` are for: the one last asked about.
    pub listing: Option<String>,
    pub items: Vec<ChangeLogSummary>,
    /// Each download by log id: the file it was saved to, or why it was not,
    /// as a sentence. A download still going has no entry.
    pub downloads: HashMap<String, Result<std::path::PathBuf, String>>,
    pending: Option<Mutex<mpsc::Receiver<Result<Vec<ChangeLogSummary>, String>>>>,
    saving: Vec<(String, Mutex<mpsc::Receiver<Result<std::path::PathBuf, String>>>)>,
}

/// Ask for a listing's stored change logs (the Publish dialog opening, or
/// the listing it publishes into changing).
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct RefreshMyChangeLogs {
    pub listing_id: String,
}

/// Save one of a listing's stored change logs to `save_to`, the file the
/// save dialog chose.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct DownloadChangeLog {
    pub listing_id: String,
    pub log_id: String,
    pub save_to: std::path::PathBuf,
}

/// Most bytes a stored change log is read to: the API keeps at most 64 KB.
const CHANGE_LOG_READ_LIMIT: u64 = 64 * 1024;

/// `id` when it is an id the API gives (letters, digits, `-` and `_`, at
/// most 128), so it goes into a URL path as it is.
fn api_id(id: &str) -> Option<&str> {
    let ok = !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    ok.then_some(id)
}

/// A listing's change logs from a `GET .../changelogs` answer, newest first;
/// an entry without a usable id is left out.
fn change_logs_from(body: &serde_json::Value) -> Vec<ChangeLogSummary> {
    let text = |v: &serde_json::Value| v.as_str().map(str::trim).unwrap_or_default().to_string();
    let mut items: Vec<ChangeLogSummary> = body
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    Some(ChangeLogSummary {
                        id: api_id(row["id"].as_str()?)?.to_string(),
                        version: row["version"].as_u64().and_then(|v| u32::try_from(v).ok()).unwrap_or(0),
                        publish_hash: text(&row["publish_hash"]),
                        created_at: text(&row["created_at"]),
                        bytes: row["bytes"].as_u64().unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    items.sort_by(|a, b| b.version.cmp(&a.version).then_with(|| b.created_at.cmp(&a.created_at)));
    items
}

/// A sentence for a change-log request the API refused or never answered:
/// `missing` for a 404, which the API gives for a log or listing this
/// account cannot read as for one that is not there.
fn change_log_failure(error: ureq::Error, doing: &str, missing: &str) -> String {
    match error {
        ureq::Error::Status(401, _) => "Sign in again to reach your change logs.".to_string(),
        ureq::Error::Status(404, _) => missing.to_string(),
        ureq::Error::Status(status, _) => format!("Could not {doing}: the API answered {status}."),
        ureq::Error::Transport(t) => format!("Could not {doing}: {t}"),
    }
}

/// Fetch one stored change log and write it to `save_to` through a sibling
/// temporary file, so a failed download leaves that file as it was.
fn download_change_log(url: &str, token: &str, save_to: &std::path::Path) -> Result<(), String> {
    use std::io::Read;
    if save_to.file_name().is_none() {
        return Err("Choose a file to save the change log to.".into());
    }
    let reply = ureq::get(url)
        .timeout(std::time::Duration::from_secs(10))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|e| change_log_failure(e, "download the change log", "That change log is not there any more."))?;
    let mut bytes = Vec::new();
    reply
        .into_reader()
        .take(CHANGE_LOG_READ_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Could not download the change log: {e}"))?;
    if bytes.len() as u64 > CHANGE_LOG_READ_LIMIT {
        return Err("That change log is larger than 64 KB, so it was not saved.".into());
    }
    crate::space::gui_loader::write_atomic(save_to, &bytes)
        .map_err(|e| format!("Could not save the change log to {}: {e}", save_to.display()))
}

/// Fetch [`MyChangeLogs`] for the listing asked about, and save the logs
/// asked for, off the frame with the signed-in token; take each answer when
/// it comes. Asking about another listing drops the answer still owed for
/// the last one, so it never shows under the new one.
fn refresh_my_change_logs(
    mut asks: MessageReader<RefreshMyChangeLogs>,
    mut saves: MessageReader<DownloadChangeLog>,
    auth: Option<Res<crate::auth::AuthState>>,
    mut logs: ResMut<MyChangeLogs>,
) {
    let answer = logs.pending.as_ref().and_then(|rx| {
        let rx = rx.lock().ok()?;
        match rx.try_recv() {
            Ok(answer) => Some(answer),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("The request for the change logs stopped without an answer.".into())),
        }
    });
    if let Some(answer) = answer {
        logs.pending = None;
        match answer {
            Ok(items) => {
                logs.items = items;
                logs.state = MyListingsState::Ready;
            }
            Err(why) => {
                logs.items.clear();
                logs.state = MyListingsState::Error(why);
            }
        }
    }
    let mut finished = Vec::new();
    logs.saving.retain(|(log_id, rx)| {
        let answer = match rx.lock() {
            Ok(rx) => match rx.try_recv() {
                Ok(answer) => answer,
                Err(mpsc::TryRecvError::Empty) => return true,
                Err(mpsc::TryRecvError::Disconnected) => Err("The download stopped without an answer.".to_string()),
            },
            Err(_) => Err("The download stopped without an answer.".to_string()),
        };
        finished.push((log_id.clone(), answer));
        false
    });
    logs.downloads.extend(finished);

    let token = auth.as_deref().and_then(|a| a.get_token()).map(str::to_string);
    if let Some(ask) = asks.read().last().cloned() {
        if logs.listing.as_deref() != Some(ask.listing_id.as_str()) {
            logs.downloads.clear();
        }
        logs.listing = Some(ask.listing_id.clone());
        logs.items.clear();
        logs.pending = None;
        match (api_id(&ask.listing_id), &token) {
            (None, _) => logs.state = MyListingsState::Error("That is not a listing the API gave.".into()),
            (_, None) => logs.state = MyListingsState::Error("Sign in to see this listing's change logs.".into()),
            (Some(id), Some(token)) => {
                let url = eustress_common::api_base::api_url(&format!("/api/simulations/{id}/changelogs"));
                let token = token.clone();
                let (tx, rx) = mpsc::channel();
                let started = std::thread::Builder::new().name("eustress-change-logs".into()).spawn(move || {
                    let answer = ureq::get(&url)
                        .timeout(std::time::Duration::from_secs(10))
                        .set("Authorization", &format!("Bearer {token}"))
                        .call()
                        .map_err(|e| change_log_failure(e, "load the change logs", "This listing has no change logs you can read."))
                        .and_then(|reply| {
                            reply.into_json::<serde_json::Value>().map_err(|e| format!("Could not read the change logs: {e}"))
                        })
                        .map(|body| change_logs_from(&body));
                    let _ = tx.send(answer);
                });
                match started {
                    Ok(_) => {
                        logs.pending = Some(Mutex::new(rx));
                        logs.state = MyListingsState::Loading;
                    }
                    Err(e) => logs.state = MyListingsState::Error(format!("Could not load the change logs: {e}")),
                }
            }
        }
    }

    for save in saves.read() {
        let log_id = save.log_id.clone();
        logs.downloads.remove(&log_id);
        let (Some(listing), Some(log), Some(token)) = (api_id(&save.listing_id), api_id(&save.log_id), &token) else {
            let why = if token.is_none() { "Sign in to download this change log." } else { "That is not a change log the API gave." };
            logs.downloads.insert(log_id, Err(why.to_string()));
            continue;
        };
        let url = eustress_common::api_base::api_url(&format!("/api/simulations/{listing}/changelogs/{log}"));
        let token = token.clone();
        let save_to = save.save_to.clone();
        let (tx, rx) = mpsc::channel();
        let started = std::thread::Builder::new().name("eustress-change-log-save".into()).spawn(move || {
            let _ = tx.send(download_change_log(&url, &token, &save_to).map(|()| save_to));
        });
        match started {
            Ok(_) => logs.saving.push((log_id, Mutex::new(rx))),
            Err(e) => {
                logs.downloads.insert(log_id, Err(format!("Could not download the change log: {e}")));
            }
        }
    }
}

/// The open Space's `Players.MaxPlayers`, the one value a session's size,
/// the Publish dialog and a publish all read (`docs/networking/MAX_PLAYERS.md`):
/// Studio's loaded Players service when it is loaded (a Space opened from the
/// world database alone included), else the Space's file.
pub fn space_max_players(world: &World) -> eustress_common::play_session::SpaceMaxPlayers {
    use crate::space::service_loader::ServiceComponent;
    let root = world.get_resource::<crate::space::SpaceRoot>().map(|r| r.0.as_path());
    match world.try_query::<&ServiceComponent>() {
        Some(mut services) => max_players_from(services.iter(world), root),
        None => max_players_from(std::iter::empty(), root),
    }
}

/// The open Space's `Players.MaxPlayers` from its loaded services (the
/// Players service's `max_players`, or `MaxPlayers`), else from the file
/// under `space_root`: the one rule [`space_max_players`] and
/// [`OpenSpaceMaxPlayers`] both read by.
fn max_players_from<'a>(
    services: impl Iterator<Item = &'a crate::space::service_loader::ServiceComponent>,
    space_root: Option<&std::path::Path>,
) -> eustress_common::play_session::SpaceMaxPlayers {
    use crate::space::service_loader::PropertyValue;
    use eustress_common::play_session::SpaceMaxPlayers;
    let mut services = services;
    let loaded = services.find(|s| s.class_name == "Players").map(|players| {
        match players.properties.get("max_players").or_else(|| players.properties.get("MaxPlayers")) {
            Some(PropertyValue::Int(n)) => Some(*n as f64),
            Some(PropertyValue::Float(v)) => Some(*v),
            _ => None,
        }
    });
    match (loaded, space_root) {
        (Some(value), _) => SpaceMaxPlayers::from_value(value),
        (None, Some(root)) => eustress_common::play_session::space_max_players(root),
        (None, None) => SpaceMaxPlayers::from_value(None),
    }
}

/// The open Space's `Players.MaxPlayers`, kept current from its loaded
/// Players service (and its file when none is loaded), for readers that have
/// no World, such as the Publish dialog. A Properties edit or
/// [`set_space_max_players`] shows here the next frame.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenSpaceMaxPlayers(pub eustress_common::play_session::SpaceMaxPlayers);

impl Default for OpenSpaceMaxPlayers {
    fn default() -> Self {
        Self(eustress_common::play_session::SpaceMaxPlayers::from_value(None))
    }
}

/// Keep [`OpenSpaceMaxPlayers`] current: when a service changes or another
/// Space opens.
fn track_space_max_players(
    services: Query<&crate::space::service_loader::ServiceComponent>,
    changed: Query<(), Changed<crate::space::service_loader::ServiceComponent>>,
    mut removed: RemovedComponents<crate::space::service_loader::ServiceComponent>,
    root: Option<Res<crate::space::SpaceRoot>>,
    mut open: ResMut<OpenSpaceMaxPlayers>,
    mut primed: Local<bool>,
) {
    let services_changed = !changed.is_empty() || removed.read().next().is_some();
    let root_changed = root.as_ref().is_some_and(|r| r.is_changed());
    if *primed && !services_changed && !root_changed {
        return;
    }
    *primed = true;
    let now = max_players_from(services.iter(), root.as_deref().map(|r| r.0.as_path()));
    if open.0 != now {
        open.0 = now;
    }
}

/// Set the open Space's `Players.MaxPlayers` to `players`, as a Properties
/// edit does: on its Players service, saved to its file and signed when
/// signed in. The Publish dialog and the MCP publish tool write it here
/// before a publish, so hosting, Properties and the listing hold one value.
pub fn set_space_max_players(world: &mut World, players: u16) -> Result<(), String> {
    use crate::space::service_loader::{save_service_to_file_signed, PropertyValue, ServiceComponent};
    use eustress_common::play_session::MAX_PLAYERS_LIMIT;
    if players == 0 || players > MAX_PLAYERS_LIMIT {
        return Err(format!("Max players must be a whole number from 1 to {MAX_PLAYERS_LIMIT}."));
    }
    let stamp = world
        .get_resource::<crate::auth::AuthState>()
        .and_then(crate::space::instance_loader::current_stamp);
    let mut services = world.query::<&mut ServiceComponent>();
    let Some(mut service) = services.iter_mut(world).find(|s| s.class_name == "Players") else {
        return Err("This Space has no Players service to hold Max players.".to_string());
    };
    // The key the file already uses, so the file never holds both.
    let key = if service.properties.contains_key("MaxPlayers") && !service.properties.contains_key("max_players") {
        "MaxPlayers"
    } else {
        "max_players"
    };
    service.properties.insert(key.to_string(), PropertyValue::Int(i64::from(players)));
    save_service_to_file_signed(&service, stamp.as_ref())
}

/// How many players a session holds, and the Output line saying why: the
/// hosted Space's `Players.MaxPlayers`, or the developer override
/// `EUSTRESS_HOST_MAX_PLAYERS` (`var`) when it holds a whole number of at
/// least 1.
fn session_size(space: eustress_common::play_session::SpaceMaxPlayers, var: Option<&str>) -> (u16, String) {
    use eustress_common::play_session::LOAD_TESTED_PLAYERS;
    if let Some(players) = var.and_then(|v| v.trim().parse::<u16>().ok()).filter(|n| *n >= 1) {
        return (
            players,
            format!(
                "This session holds {players} players: EUSTRESS_HOST_MAX_PLAYERS overrides this Space's \
                 Players.MaxPlayers ({}). The override is for development; unset it to use the Space's.",
                space.players
            ),
        );
    }
    let source = match space.authored {
        Some(_) => "this Space's Players.MaxPlayers".to_string(),
        None => "Players.MaxPlayers is unset, so the default".to_string(),
    };
    let mut line = format!(
        "This session holds {} players, the host's own character included while it plays ({source}).",
        space.players
    );
    if space.untested() {
        line.push_str(&format!(" Sessions above {LOAD_TESTED_PLAYERS} players haven't been load-tested yet."));
    }
    (space.players, line)
}

/// Tell the session each frame whether the host's own character is in it:
/// it counts as one of the Space's MaxPlayers.
fn track_host_character(
    host: Option<ResMut<HostSession>>,
    local: Query<(), (With<eustress_common::avatar::LocalAvatar>, With<eustress_common::avatar::SpawnedByAvatarRuntime>)>,
) {
    if let Some(mut host) = host {
        let plays = !local.is_empty();
        if host.host_plays() != plays {
            host.set_host_plays(plays);
        }
    }
}

/// Read the served Space's character policy (`SpaceCharacterPolicy`: its
/// StarterPlayer switches and characters) from its files, as its Players
/// read it from the world they download, so the host measures and dresses
/// players by the Space it serves, never by one a previous Play left.
#[cfg_attr(not(feature = "world-db"), allow(dead_code))]
fn refresh_character_policy(world: &mut World, space_root: &std::path::Path) {
    let (policy, warnings) = eustress_common::avatar::space_character::SpaceCharacterPolicy::load(space_root);
    for w in &warnings {
        warn!("avatar: {w}");
    }
    world.insert_resource(policy);
}

#[cfg(feature = "world-db")]
fn prepare_export(world: &mut World) -> Result<PendingExport, String> {
    use crate::space::echk_export::{export_world, Audience, SpaceInput, SpaceSource, SAVE_SETTLE};

    let space_root = world
        .get_resource::<crate::space::SpaceRoot>()
        .map(|r| r.0.clone())
        .ok_or("Open a Space before starting a server.")?;
    let still_opening = world
        .get_resource::<crate::space::world_db_plugin::PendingWorldDbOpen>()
        .is_some_and(|p| p.0.is_some());
    if still_opening {
        return Err("The Space is still opening. Start the server again in a moment.".into());
    }
    let db = crate::space::active_db::db_arc()
        .ok_or("This Space has no database open, so it cannot be served. Reopen the Space and try again.")?;

    let editing = world
        .get_resource::<State<PlayModeState>>()
        .map(|s| *s.get() == PlayModeState::Editing)
        .unwrap_or(true);
    // Players get what the host sees. The terrain comes from memory, so
    // hosting never writes the Space's terrain files; a Space whose terrain
    // cannot be taken that way (a migrated one) hosts its saved terrain. Some
    // edits (a rotate, an undo) reach disk, and so the tree the bake reads,
    // only through a save, so unsaved edits are flushed first: the parts whose
    // live values differ from their files. Hosting is not a save: no commit,
    // no toast. With nothing unsaved the bake already reads the host's world,
    // and flushing anyway held the frame for seconds on a large Space.
    let terrain = crate::ui::file_event_handler::terrain_snapshot(world);
    let stale_terrain =
        terrain.is_none() && editing && crate::ui::file_event_handler::terrain_changed_since_save(world, false);
    let saved = editing && has_unsaved_edits(world);
    if saved {
        // A refused flush wrote nothing (a snapshot revert is pending), and a
        // Space mid-revert is not served.
        crate::ui::file_event_handler::flush_space(world, crate::ui::file_event_handler::FlushTerrain::Skip)
            .map_err(|e| format!("Could not start the server: {e}"))?;
    }
    // A Play reads the Space's character policy as it begins; a server
    // started from Edit reads it here, before any session exists, from the
    // Space it serves.
    if editing {
        refresh_character_policy(world, &space_root);
    }

    let spawn = host_spawn_point(world);
    let max_players = space_max_players(world);
    let host_name = world
        .get_resource::<crate::auth::AuthState>()
        .and_then(|a| a.user.as_ref().map(|u| u.username.clone()))
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "Host".to_string());
    let universe_root = crate::space::universe_root_for_path(&space_root).unwrap_or_else(|| space_root.clone());
    let universe = folder_name(&universe_root);
    let space = folder_name(&space_root);
    // Where purchases in this session are made, and which listing the gallery
    // shows it under: the listing publishing keeps in the Universe's
    // sync.toml for the API in use.
    let sim_id = eustress_common::load_toml_file::<eustress_common::SyncManifest>(
        &universe_root.join(".eustress").join("sync.toml"),
    )
    .ok()
    .and_then(|s| s.remote.listing_id().map(str::to_string));
    let threshold = world
        .get_resource::<crate::space::residency::ResidencyConfig>()
        .map(|c| c.big_space_threshold)
        .unwrap_or(100_000);
    let out_root = host_cache_dir(&universe_root);
    // How this host will listen, read once: the listener binds by it, and the
    // export screens for the same reach.
    let options = host_options();
    let destination = export_destination(&options);

    let (tx, rx) = mpsc::channel();
    let space_name = space.clone();
    let keys_root = space_root.clone();
    std::thread::Builder::new()
        .name("eustress-host-bake".into())
        .spawn(move || {
            if saved {
                std::thread::sleep(SAVE_SETTLE);
            }
            // Encoding a large terrain takes a while, so it happens here.
            let terrain = terrain.and_then(|snapshot| match snapshot.encode() {
                Ok(files) => Some(files),
                Err(e) => {
                    warn!("multiplayer: the live terrain could not be encoded ({e}); players get the terrain as last saved");
                    None
                }
            });
            let inputs = vec![SpaceInput {
                name: space_name.clone(),
                source: SpaceSource::Db(db),
                folder: Some(space_root),
                terrain,
            }];
            // The Universe's shared assets travel too: a Space's meshes and
            // textures can live there.
            // Players get what Roblox would replicate to them: the
            // server-only services stay on this machine.
            let result = export_world(
                &universe,
                inputs,
                &space_name,
                Some(universe_root.as_path()),
                &out_root,
                threshold,
                Audience::Players,
                destination,
            )
            .and_then(|exported| {
                for (space, stats) in &exported.stats {
                    if !stats.webhook_paths.is_empty() {
                        warn!(
                            "multiplayer: {space}: players will receive {} file(s) holding a webhook URL: {}",
                            stats.webhook_paths.len(),
                            stats.webhook_paths.join(", ")
                        );
                    }
                }
                // Replication names a streamed core by the key players load
                // it from, which only the export knows.
                let cores: HashMap<u64, String> = exported
                    .stats
                    .iter()
                    .filter(|(s, _)| *s == space_name)
                    .flat_map(|(_, st)| st.core_keys.iter().cloned())
                    .collect();
                let chunks = exported.load_all()?;
                // Hosting a session already under way: what players load,
                // laid out as the Player reads it, so a late joiner gets
                // what the session changed before now.
                let layout = if editing { None } else { world_layout(&exported.manifest, &chunks, &space_name) };
                // SoulService's notice on a LAN host, then the key warnings
                // (the notice itself is one on a loopback host).
                let notes: Vec<String> =
                    exported.notice.iter().map(|notice| notice.to_string()).chain(exported.warnings.iter().cloned()).collect();
                Ok((exported.manifest, chunks, cores, layout, notes))
            });
            let _ = tx.send(result);
        })
        .map_err(|e| format!("Could not start the world bake: {e}"))?;

    if stale_terrain {
        notify(
            world,
            Say::Info,
            "Players get the terrain as it was last saved. Save to include this session's terrain edits.".into(),
        );
    }
    Ok(PendingExport {
        options,
        rx: Mutex::new(rx),
        space,
        space_root: keys_root,
        host_name,
        spawn,
        start_play: editing,
        sim_id,
        max_players,
    })
}

/// Edits since the last save or autosave, as the title's asterisk counts
/// them, or terrain changed since it was last written.
#[cfg(feature = "world-db")]
fn has_unsaved_edits(world: &mut World) -> bool {
    let sequence = world.get_resource::<crate::undo::UndoStack>().map(|undo| undo.sequence());
    let unsaved = match (world.get_resource::<crate::ui::StudioState>(), sequence) {
        (Some(state), Some(sequence)) => state.has_unsaved_changes || state.saved_undo_sequence != sequence,
        _ => true,
    };
    unsaved || crate::ui::file_event_handler::terrain_changed_since_save(world, false)
}

#[cfg(not(feature = "world-db"))]
fn prepare_export(_world: &mut World) -> Result<PendingExport, String> {
    Err("Hosting serves the Space's database, and this build has no world-db feature.".into())
}

/// Where players' feet go: on top of the Space's SpawnLocation, else the
/// PlayerService default. The same rule Play uses for the host's own character.
fn host_spawn_point(world: &mut World) -> Vec3 {
    let default = world
        .get_resource::<eustress_common::services::PlayerService>()
        .map(|p| p.spawn_position)
        .unwrap_or(Vec3::ZERO);
    let mut spawns = world.query::<(&Transform, &eustress_common::classes::SpawnLocation)>();
    eustress_common::services::spawn_feet_position(spawns.iter(world), None, default).0
}

/// The start Space as a joining player reads it (see `WorldLayout`), from
/// the baked world. Runs on the bake thread, never the frame.
fn world_layout(
    manifest: &WorldManifest,
    chunks: &HashMap<String, Arc<Vec<u8>>>,
    space: &str,
) -> Option<eustress_networking::repl::WorldLayout> {
    let started = std::time::Instant::now();
    let world = eustress_networking::session::assemble_world(manifest, |h| chunks.get(h).map(|b| b.as_slice()), [0.0; 3])
        .map_err(|e| warn!("multiplayer: the world could not be laid out for late joiners ({e})"))
        .ok()?;
    let (_, records) = world.spaces.iter().find(|(name, _)| name == space)?;
    let layout = eustress_networking::repl::WorldLayout::of_records(records);
    info!(
        "multiplayer: laid out {} scene instances for late joiners in {} ms (bake thread)",
        layout.len(),
        started.elapsed().as_millis()
    );
    Some(layout)
}

/// Where hosting writes the world it serves: a cache in the Eustress
/// workspace's own `.eustress/host/`, one folder per Universe (its name and a
/// hash of its path), never inside a Space or Universe folder, so no Space's
/// autosave commits it, wherever the Universe lives.
fn host_cache_dir(universe_root: &std::path::Path) -> std::path::PathBuf {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in universe_root.to_string_lossy().bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let name = format!("{}-{hash:016x}", folder_name(universe_root));
    crate::space::workspace_root().join(".eustress").join("host").join(name)
}

fn folder_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".to_string())
}

/// When the bake finishes, start listening and enter Play.
fn poll_host_export(
    mut commands: Commands,
    mut request: ResMut<HostRequest>,
    mut start_play: MessageWriter<StartPlayEvent>,
    mut notifications: Option<ResMut<NotificationManager>>,
    mut output: Option<ResMut<OutputConsole>>,
) {
    let result = {
        let Some(pending) = &request.pending else { return };
        let Ok(rx) = pending.rx.lock() else { return };
        match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("the world bake stopped without a result".into()),
        }
    };
    let pending = request.pending.take().expect("checked above");

    let (manifest, chunks, cores, layout, notes) = match result {
        Ok(baked) => baked,
        Err(e) => {
            error!("multiplayer: could not bake {} for hosting: {e}", pending.space);
            if let Some(n) = notifications.as_mut() {
                n.error(format!("Could not start the server: {e}"));
            }
            return;
        }
    };
    // What the export's screen asks the host to read, each line a warning in
    // the Output: SoulService's notice, and each key players will receive.
    for line in &notes {
        warn!("multiplayer: {line}");
        if let Some(out) = output.as_mut() {
            out.warn(line.clone());
        }
    }

    // The options the bake was screened for: the listener binds by the same
    // value.
    let options = pending.options.clone();
    let link = match native::start_host(options.clone()) {
        Ok(link) => link,
        Err(e) => {
            error!("multiplayer: {e}");
            if let Some(n) = notifications.as_mut() {
                n.error(format!("Could not start the server: {e}"));
            }
            return;
        }
    };
    let bytes: u64 = manifest.download_bytes();
    info!(
        "multiplayer: serving {} ({} chunks, {} bytes) as {}",
        pending.space,
        chunks.len(),
        bytes,
        pending.host_name
    );
    commands.insert_resource(crate::net_replicate::HostWorldKeys {
        space_root: pending.space_root.clone(),
        cores,
        world: layout.map(Arc::new),
    });
    begin_host(
        &mut commands,
        link,
        HostConfig {
            host_name: pending.host_name,
            max_players: {
                let (players, line) =
                    session_size(pending.max_players, std::env::var("EUSTRESS_HOST_MAX_PLAYERS").ok().as_deref());
                info!("multiplayer: {line}");
                if let Some(out) = output.as_mut() {
                    out.info(line);
                }
                players
            },
            world: manifest,
            chunks,
            spawn: pending.spawn.to_array(),
            sim_id: pending.sim_id,
        },
    );
    request.active = Some(ActiveHost { join_key: options.join_key, lan: options.lan, port: 0, pin: None, saw_playing: false });
    if pending.start_play {
        start_play.write(StartPlayEvent { play_type: PlayModeType::WithCharacter });
    }
}

/// Returning to Edit ends the session. Pausing does not.
fn stop_when_play_ends(
    state: Option<Res<State<PlayModeState>>>,
    host: Option<Res<HostSession>>,
    mut request: ResMut<HostRequest>,
    mut end: MessageWriter<EndSession>,
) {
    let (Some(state), Some(_host)) = (state, host) else { return };
    let Some(active) = request.active.as_mut() else { return };
    match state.get() {
        PlayModeState::Playing | PlayModeState::Paused => active.saw_playing = true,
        PlayModeState::Editing => {
            if active.saw_playing {
                active.saw_playing = false;
                end.write(EndSession { reason: "the host stopped playing".into() });
            }
        }
    }
}

/// Tell the person at the keyboard what the session is doing.
fn report_net_notices(
    mut notices: MessageReader<NetNotice>,
    mut request: ResMut<HostRequest>,
    mut notifications: Option<ResMut<NotificationManager>>,
    mut output: Option<ResMut<OutputConsole>>,
) {
    for notice in notices.read() {
        match notice {
            NetNotice::Hosting { port, pin } => {
                let Some(active) = request.active.as_mut() else { continue };
                active.port = *port;
                active.pin = Some(*pin);
                let link = join_link(active, Some(*pin));
                let text = link.to_link();
                native::write_host_file(&eustress_bridge_client::default_workspace_root(), &link);
                let reach = if active.lan { "on your local network" } else { "on this computer" };
                info!("multiplayer: hosting {reach}; join with {text}");
                if let Some(n) = notifications.as_mut() {
                    n.success(format!(
                        "Hosting {reach} on port {port}. A Player on this computer joins with: eustress-client --connect 127.0.0.1:{port}"
                    ));
                }
                if let Some(o) = output.as_mut() {
                    o.info(format!("Join link: {text}"));
                    o.info(format!("On this computer: eustress-client --connect 127.0.0.1:{port}"));
                }
            }
            NetNotice::HostEnded { reason } => {
                if let Some(active) = request.active.take() {
                    if active.port != 0 {
                        native::remove_host_file(&eustress_bridge_client::default_workspace_root(), active.port);
                    }
                }
                if let Some(n) = notifications.as_mut() {
                    n.info(format!("Server stopped: {reason}."));
                }
            }
            NetNotice::PeerJoined { name, .. } => {
                if let Some(n) = notifications.as_mut() {
                    n.info(format!("{name} joined."));
                }
                if let Some(o) = output.as_mut() {
                    o.info(format!("{name} joined the session."));
                }
            }
            NetNotice::PeerLeft { name, .. } => {
                if let Some(n) = notifications.as_mut() {
                    n.info(format!("{name} left."));
                }
                if let Some(o) = output.as_mut() {
                    o.info(format!("{name} left the session."));
                }
            }
            NetNotice::Chat { name, text, .. } => {
                if let Some(o) = output.as_mut() {
                    o.info(format!("[{name}] {text}"));
                }
            }
            _ => {}
        }
    }
}

/// The Output's line for a hosting Space's movement check, when it has one
/// to say: a Space that turns the check off (StarterPlayer
/// `ServerMovementCheck = false`) has the host take any movement a joined
/// player sends.
fn movement_check_line(on: bool, was_off: bool) -> Option<(bool, String)> {
    match (on, was_off) {
        (false, _) => Some((
            true,
            "This Space turns the movement check off (StarterPlayer ServerMovementCheck): the host takes any movement \
             a joined player sends, so a modified Player can move anywhere. Keep it off only if this Space's own \
             player scripts move characters faster than they walk."
                .to_string(),
        )),
        (true, true) => Some((false, "The movement check is on again: the host refuses movement faster than a player's character.".to_string())),
        (true, false) => None,
    }
}

/// While hosting, say in the Output when the Space turns the movement check
/// off, once per change. The setting is the host's own Space's
/// (`SpaceCharacterPolicy`, read from its StarterPlayer), never a player's.
fn report_movement_check(
    request: Res<HostRequest>,
    policy: Option<Res<eustress_common::avatar::space_character::SpaceCharacterPolicy>>,
    mut output: Option<ResMut<OutputConsole>>,
    mut told: Local<Option<bool>>,
) {
    if request.active.is_none() {
        *told = None;
        return;
    }
    let on = policy.as_deref().map_or(true, |p| p.movement_check());
    if *told == Some(on) {
        return;
    }
    let was_off = *told == Some(false);
    *told = Some(on);
    let Some((warning, line)) = movement_check_line(on, was_off) else { return };
    if warning {
        warn!("multiplayer: {line}");
    } else {
        info!("multiplayer: {line}");
    }
    if let Some(out) = output.as_mut() {
        if warning {
            out.warn(line);
        } else {
            out.info(line);
        }
    }
}

/// Write this host's file again every [`native::HOST_FILE_REFRESH`] while it
/// hosts. A Player on this machine then tells a running host's file from one
/// a killed Studio left behind ([`native::HOST_FILE_FRESH`]). Real time, so a
/// paused session keeps its file current.
fn refresh_host_file(time: Res<Time<Real>>, request: Res<HostRequest>, mut next_at: Local<f64>) {
    let Some(active) = request.active.as_ref().filter(|a| a.port != 0 && a.pin.is_some()) else {
        return;
    };
    let now = time.elapsed_secs_f64();
    if now < *next_at {
        return;
    }
    *next_at = now + native::HOST_FILE_REFRESH.as_secs_f64();
    native::write_host_file(&eustress_bridge_client::default_workspace_root(), &join_link(active, active.pin));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host_config(sim_id: Option<&str>) -> HostConfig {
        HostConfig {
            host_name: "Host".into(),
            max_players: 8,
            world: WorldManifest::new("U", "0", 256.0),
            chunks: HashMap::new(),
            spawn: [0.0, 1.0, 0.0],
            sim_id: sim_id.map(str::to_string),
        }
    }

    /// The real heartbeat system, hosting, a heartbeat already sent (number
    /// 1, so joins wait for the gallery), and the gallery's answer to it in.
    /// The session is not listening yet, so it sends nothing more.
    fn heartbeat_app(answer: BeatOutcome, maybe_listed: bool) -> App {
        let mut host = HostSession::new(host_config(Some("sim-1")));
        admission_for_sent_beat(&mut host);
        let mut beat = GalleryBeat::default();
        beat.maybe_listed = maybe_listed;
        beat.sent = 1;
        beat.unanswered.insert(1);
        beat.answers.lock().unwrap().push((1, answer));
        let hosting = ActiveHost { join_key: "key".into(), lan: false, port: 7777, pin: None, saw_playing: false };
        let mut app = App::new();
        app.init_resource::<Time>()
            .insert_resource(HostRequest { active: Some(hosting), ..Default::default() })
            .init_resource::<OutputConsole>()
            .insert_resource(host)
            .insert_resource(beat)
            .add_systems(Update, gallery_heartbeat);
        app
    }

    /// Joins wait for the gallery's admission from the moment a heartbeat
    /// that lists the link goes out. A session the gallery may never have
    /// listed opens again only when the gallery certainly stored nothing (a
    /// refusal, or a request that never left); after an answer that never
    /// came, a server error, or once maybe listed, it stays. Each answer
    /// reaches the Output once.
    #[test]
    fn admission_starts_at_the_send_and_ends_only_when_nothing_was_stored() {
        let mut host = HostSession::new(host_config(Some("sim-1")));
        assert!(!host.requires_admission());
        admission_for_sent_beat(&mut host);
        assert!(host.requires_admission(), "required the moment the link goes out");

        let refused = |status: u16| BeatOutcome::of(Err(BeatFailure::Status(status, String::new())));
        for (answer, maybe_listed, admission) in [
            (BeatOutcome::of(Ok(())), false, true),
            (BeatOutcome::of(Err(BeatFailure::Unanswered("timed out".into()))), false, true),
            (refused(503), false, true),
            (refused(403), false, false),
            (refused(404), false, false),
            (refused(400), false, false),
            (BeatOutcome::of(Err(BeatFailure::Unreached("no address".into()))), false, false),
            (refused(403), true, true),
            (refused(401), true, true),
        ] {
            let mut app = heartbeat_app(answer.clone(), maybe_listed);
            app.update();
            assert_eq!(
                app.world().resource::<HostSession>().requires_admission(),
                admission,
                "{answer:?}, maybe listed before: {maybe_listed}"
            );
            app.world().resource::<GalleryBeat>().answers.lock().unwrap().push((1, answer.clone()));
            app.update();
            let shown = &app.world().resource::<OutputConsole>().entries;
            assert_eq!(shown.len(), 1, "shown once, though answered twice: {answer:?}");
            assert_eq!(shown[0].message, answer.message());
        }
    }

    /// One hosting session's heartbeats in order: each that went out was sent
    /// (joins wait), then answered. Whether joins still wait afterwards.
    fn after_beats(outcomes: &[BeatOutcome]) -> bool {
        let mut host = HostSession::new(host_config(Some("sim-1")));
        let (mut maybe_listed, mut unanswered) = (false, BTreeSet::new());
        for (i, outcome) in outcomes.iter().enumerate() {
            let number = if matches!(outcome, BeatOutcome::NotSent(_)) {
                0
            } else {
                admission_for_sent_beat(&mut host);
                unanswered.insert(i as u64 + 1);
                i as u64 + 1
            };
            admission_after_answer(&mut host, &mut maybe_listed, &mut unanswered, number, outcome);
        }
        host.requires_admission()
    }

    /// Once the gallery may have stored the link (ok, no answer, a server
    /// error), no later heartbeat reopens the session, since the gallery
    /// shows a stored link for 90 s: not a lost network, an expired author
    /// token or a sign-out. Only a session it certainly never stored opens.
    #[test]
    fn a_maybe_listed_session_keeps_admission() {
        let refused = |status: u16| BeatOutcome::of(Err(BeatFailure::Status(status, String::new())));
        let timeout = || BeatOutcome::of(Err(BeatFailure::Unanswered("timed out".into())));
        let unreached = || BeatOutcome::of(Err(BeatFailure::Unreached("no address".into())));
        let signed_out = || BeatOutcome::NotSent("sign in as the listing's author".into());
        for beats in [
            vec![timeout(), unreached()],
            vec![refused(503), refused(403)],
            vec![timeout(), signed_out()],
            vec![BeatOutcome::Live, unreached()],
            vec![BeatOutcome::Live, refused(401)],
            vec![refused(403), timeout()],
            vec![refused(403), timeout(), refused(403)],
        ] {
            assert!(after_beats(&beats), "joins must still wait after {beats:?}");
        }
        for beats in [vec![refused(403), refused(403)], vec![unreached(), refused(404)], vec![refused(400)]] {
            assert!(!after_beats(&beats), "nothing was ever stored: {beats:?}");
        }
    }

    /// Heartbeats overlap: a second goes out before the first is answered.
    /// The first answer, a refusal, leaves joins waiting, since the second
    /// may store the link; once both are answered and neither stored it,
    /// joins open. An answer to a heartbeat from before this hosting session
    /// never opens it, and one that may have stored the link, whenever it
    /// comes, keeps admission.
    #[test]
    fn an_earlier_heartbeats_answer_never_releases_admission() {
        let refused = || BeatOutcome::of(Err(BeatFailure::Status(403, String::new())));
        let mut host = HostSession::new(host_config(Some("sim-1")));
        let (mut maybe_listed, mut unanswered) = (false, BTreeSet::new());
        for number in [1, 2] {
            admission_for_sent_beat(&mut host);
            unanswered.insert(number);
        }
        admission_after_answer(&mut host, &mut maybe_listed, &mut unanswered, 1, &refused());
        assert!(host.requires_admission(), "the second heartbeat is still out");
        admission_after_answer(&mut host, &mut maybe_listed, &mut unanswered, 2, &refused());
        assert!(!host.requires_admission(), "both answered, nothing stored");

        // The next hosting session sends heartbeat 3; heartbeat 2's answer
        // comes again, late.
        unanswered.clear();
        admission_for_sent_beat(&mut host);
        unanswered.insert(3);
        admission_after_answer(&mut host, &mut maybe_listed, &mut unanswered, 2, &refused());
        assert!(host.requires_admission(), "an answer from before this session");
        admission_after_answer(&mut host, &mut maybe_listed, &mut unanswered, 1, &BeatOutcome::Live);
        admission_after_answer(&mut host, &mut maybe_listed, &mut unanswered, 3, &refused());
        assert!(host.requires_admission(), "maybe listed, for good");
    }

    /// Answers that arrive between two frames are each read: an ok behind a
    /// later refusal still keeps admission, and both reach the Output.
    #[test]
    fn answers_arriving_together_are_each_read() {
        let mut app = heartbeat_app(BeatOutcome::Live, false);
        {
            let mut beat = app.world_mut().resource_mut::<GalleryBeat>();
            beat.sent = 2;
            beat.unanswered.insert(2);
            beat.answers.lock().unwrap().push((2, BeatOutcome::of(Err(BeatFailure::Status(403, String::new())))));
        }
        app.update();
        assert!(app.world().resource::<HostSession>().requires_admission());
        let beat = app.world().resource::<GalleryBeat>();
        assert!(beat.maybe_listed && beat.unanswered.is_empty());
        assert_eq!(app.world().resource::<OutputConsole>().entries.len(), 2);
    }

    /// While hosting, the Output says plainly when the Space turns the
    /// movement check off, once, and again when it comes back on; a Space
    /// that leaves it on shows nothing.
    #[test]
    fn the_output_says_when_a_space_turns_the_movement_check_off() {
        use eustress_common::avatar::space_character::SpaceCharacterPolicy;
        let app_with = |setting: Option<bool>| {
            let hosting = ActiveHost { join_key: "key".into(), lan: false, port: 7777, pin: None, saw_playing: false };
            let mut app = App::new();
            app.insert_resource(HostRequest { active: Some(hosting), ..Default::default() })
                .init_resource::<OutputConsole>()
                .insert_resource(SpaceCharacterPolicy { server_movement_check: setting, ..Default::default() })
                .add_systems(Update, report_movement_check);
            app
        };
        let shown = |app: &App| app.world().resource::<OutputConsole>().entries.iter().map(|e| e.message.clone()).collect::<Vec<_>>();

        let mut quiet = app_with(None);
        quiet.update();
        assert!(shown(&quiet).is_empty(), "on by default, nothing to say");

        let mut off = app_with(Some(false));
        off.update();
        off.update();
        let lines = shown(&off);
        assert_eq!(lines.len(), 1, "said once: {lines:?}");
        assert!(lines[0].contains("ServerMovementCheck") && lines[0].contains("can move anywhere"));
        off.world_mut().resource_mut::<SpaceCharacterPolicy>().server_movement_check = Some(true);
        off.update();
        let lines = shown(&off);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].contains("on again"));
    }

    /// A session holds the hosted Space's Players.MaxPlayers, everyone
    /// counted, and the Output says so; past the load-tested size it adds a
    /// note and holds the number anyway; the developer override says it is
    /// one.
    #[test]
    fn a_session_holds_its_spaces_max_players() {
        use eustress_common::play_session::SpaceMaxPlayers;
        let four = SpaceMaxPlayers::from_value(Some(4.0));
        let (players, line) = session_size(four, None);
        assert_eq!(players, 4);
        assert!(line.contains("holds 4 players") && line.contains("this Space's Players.MaxPlayers"), "{line}");
        assert!(!line.contains("load-tested"), "{line}");
        let (players, line) = session_size(SpaceMaxPlayers::from_value(Some(12.0)), None);
        assert_eq!(players, 12, "never lowered");
        assert!(line.contains("haven't been load-tested yet"), "{line}");
        let (players, line) = session_size(SpaceMaxPlayers::from_value(None), None);
        assert_eq!(players, eustress_common::play_session::DEFAULT_MAX_PLAYERS);
        assert!(line.contains("unset, so the default"), "{line}");
        let (players, line) = session_size(four, Some(" 6 "));
        assert_eq!(players, 6);
        assert!(line.contains("EUSTRESS_HOST_MAX_PLAYERS overrides") && line.contains("for development"), "{line}");
        for ignored in ["0", "many", ""] {
            assert_eq!(session_size(four, Some(ignored)).0, 4, "{ignored:?}");
        }
    }

    /// Studio reads and writes the open Space's MaxPlayers on its Players
    /// service, as a Properties edit does, and refuses a size the gallery
    /// cannot carry.
    #[test]
    fn studio_reads_and_writes_a_spaces_max_players() {
        use crate::space::service_loader::{PropertyValue, ServiceComponent};
        let root = std::env::temp_dir().join(format!("eustress-max-players-studio-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Players")).unwrap();
        let file = root.join("Players").join("_service.toml");
        std::fs::write(&file, "[service]\nclass_name = \"Players\"\n\n[properties]\ncharacter_auto_loads = true\nmax_players = 12\n").unwrap();
        let mut world = World::new();
        let mut properties = std::collections::HashMap::new();
        properties.insert("character_auto_loads".to_string(), PropertyValue::Bool(true));
        properties.insert("max_players".to_string(), PropertyValue::Int(12));
        world.spawn(ServiceComponent { class_name: "Players".into(), toml_path: file.clone(), properties, ..Default::default() });
        assert_eq!(space_max_players(&world).players, 12);

        set_space_max_players(&mut world, 4).unwrap();
        assert_eq!(space_max_players(&world).players, 4);
        assert_eq!(eustress_common::play_session::space_max_players(&root).players, 4, "saved to the Space's file");
        assert!(std::fs::read_to_string(&file).unwrap().contains("character_auto_loads = true"), "the rest kept");
        assert!(set_space_max_players(&mut world, 0).is_err());
        assert!(set_space_max_players(&mut world, eustress_common::play_session::MAX_PLAYERS_LIMIT + 1).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Publish As lists the author's own listings from GET /api/projects,
    /// newest first, and knows the Universe's own listing on the API in use.
    #[test]
    fn publish_as_lists_the_authors_listings() {
        let body = serde_json::json!({ "projects": [
            { "id": "a1", "name": "Box Head", "status": "published", "last_edited": "2026-09-30T10:00:00Z", "thumbnail_url": "https://x/t.png", "tags": ["zombies", "tycoon"], "tags_source": "judge", "cover_url": "https://x/c?v=1" },
            { "id": "b2", "name": "", "status": "in_review", "published_at": "2026-10-01T09:00:00Z", "tags_source": "author" },
            { "name": "No id" },
        ], "total": 3 });
        let items = listings_from(&body);
        assert_eq!(items.len(), 2, "an entry with no id is left out");
        assert_eq!((items[0].id.as_str(), items[0].title.as_str(), items[0].status.as_str()), ("b2", "Untitled", "in_review"));
        assert_eq!(items[1].thumbnail_url.as_deref(), Some("https://x/t.png"));
        assert_eq!(items[1].tags, vec!["zombies".to_string(), "tycoon".to_string()]);
        assert!(items[0].tags.is_empty(), "no tags reads as none");
        assert!(items[1].tags_suggested, "the review's tags are suggestions");
        assert!(!items[0].tags_suggested, "the author's are not");
        assert_eq!(items[1].cover_url.as_deref(), Some("https://x/c?v=1"));
        assert_eq!(items[0].cover_url, None);
        assert!(listings_from(&serde_json::json!({ "error": "nope" })).is_empty());

        let root = std::env::temp_dir().join(format!("eustress-own-listing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".eustress")).unwrap();
        assert_eq!(own_listing(&root), None);
        let mut sync = eustress_common::SyncManifest::default();
        sync.remote.set_listing_id(Some("mine".into()));
        eustress_common::save_toml_file(&sync, &root.join(".eustress").join("sync.toml")).unwrap();
        assert_eq!(own_listing(&root).as_deref(), Some("mine"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The Publish dialog's copy follows the Players service: a Properties
    /// edit shows the next frame, and with no Players service the default
    /// stands.
    #[test]
    fn the_open_spaces_max_players_follows_its_players_service() {
        use crate::space::service_loader::{PropertyValue, ServiceComponent};
        let mut app = App::new();
        app.init_resource::<OpenSpaceMaxPlayers>().add_systems(Update, track_space_max_players);
        let held = |app: &App| app.world().resource::<OpenSpaceMaxPlayers>().0.players;
        let mut properties = HashMap::new();
        properties.insert("max_players".to_string(), PropertyValue::Int(4));
        let players = app
            .world_mut()
            .spawn(ServiceComponent { class_name: "Players".into(), properties, ..Default::default() })
            .id();
        app.update();
        assert_eq!(held(&app), 4);
        app.world_mut()
            .get_mut::<ServiceComponent>(players)
            .unwrap()
            .properties
            .insert("max_players".to_string(), PropertyValue::Int(6));
        app.update();
        assert_eq!(held(&app), 6, "a Properties edit");
        app.world_mut().despawn(players);
        app.update();
        assert_eq!(held(&app), eustress_common::play_session::DEFAULT_MAX_PLAYERS, "no Players service");
    }

    /// A server started from Edit measures players by the Space it serves:
    /// the policy a previous Play left (here, another Space's with the check
    /// off) is replaced by the served Space's own.
    #[test]
    fn hosting_reads_the_served_spaces_character_policy() {
        use eustress_common::avatar::space_character::{starter_player_file, SpaceCharacterPolicy};
        let root = std::env::temp_dir().join(format!("eustress-host-policy-{}", std::process::id()));
        std::fs::create_dir_all(root.join("StarterPlayer")).unwrap();
        std::fs::write(starter_player_file(&root), "[properties]\nstandard_player_bodies = true\n").unwrap();
        let mut world = World::new();
        world.insert_resource(SpaceCharacterPolicy { server_movement_check: Some(false), ..Default::default() });
        refresh_character_policy(&mut world, &root);
        let policy = world.resource::<SpaceCharacterPolicy>();
        assert!(policy.movement_check(), "the served Space leaves the check on");
        assert!(policy.standard_player_bodies, "and asks for standard bodies");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The listener and the export read one value: a host whose options bind
    /// every interface screens its export as leaving this machine, and one
    /// bound to loopback as staying.
    #[test]
    fn the_export_destination_comes_from_the_host_options() {
        for lan in [true, false] {
            let options = HostOptions { port: native::DEFAULT_PORT, lan, join_key: native::new_join_key() };
            let expected = if lan { Destination::LanHost } else { Destination::LoopbackHost };
            assert_eq!(export_destination(&options), expected, "lan = {lan}");
            assert_eq!(export_destination(&options).leaves_machine(), options.lan, "lan = {lan}");
        }
    }

    /// Why the gallery lists nothing names what to do.
    #[test]
    fn the_gallery_says_why_it_lists_nothing() {
        let lan_ip = Some("192.168.1.20");
        let unpublished = unlisted_because(false, lan_ip, true).unwrap();
        assert!(unpublished.contains("publish it there"), "{unpublished}");
        assert!(unlisted_because(true, None, true).unwrap().contains("EUSTRESS_HOST_PUBLIC"));
        assert!(unlisted_because(true, lan_ip, false).unwrap().contains("EUSTRESS_HOST_LAN=1"));
        assert_eq!(unlisted_because(true, lan_ip, true), None);
        assert_eq!(unlisted_because(true, Some("play.example.com"), true), None, "a name is listed");
        // A host listens on IPv4 only: an IPv6 link would list a session
        // nobody reaches, so it is not listed and the Output says why.
        for v6 in ["[2001:db8::7]", "2001:db8::7", "[::ffff:192.168.1.20]"] {
            let why = unlisted_because(true, Some(v6), true).unwrap();
            assert!(why.contains("IPv6") && why.contains("IPv4"), "{v6}: {why}");
        }
        let missing = BeatOutcome::of(Err(BeatFailure::Status(404, r#"{"error":"Simulation not found"}"#.into())));
        assert!(missing.message().contains("publish the Universe to it first"), "{}", missing.message());
        let other = BeatOutcome::of(Err(BeatFailure::Status(403, String::new())));
        assert!(other.message().contains("sign in as the author"), "{}", other.message());
    }

    /// Quitting tells the gallery only about a session it may show, and only
    /// with a sign-in to tell it with.
    #[test]
    fn quitting_stops_only_a_listed_session() {
        let mut beat = GalleryBeat::default();
        assert_eq!(listing_to_stop(&beat, Some("jwt")), None, "nothing told, nothing to stop");
        beat.told = Some(("sim-1".into(), "eustress-player://join/x".into()));
        assert_eq!(listing_to_stop(&beat, None), None, "signed out: no request");
        assert_eq!(listing_to_stop(&beat, Some("jwt")), Some(("sim-1".to_string(), "jwt".to_string())));
    }

    // ── change logs ──

    #[test]
    fn the_change_logs_list_newest_first_with_usable_ids_only() {
        let body = serde_json::json!([
            { "id": "0000000006-aaaaaaaa-0", "version": 6, "publish_hash": "h6", "created_at": "2026-10-01T10:00:00.000Z", "bytes": 900 },
            { "id": "0000000007-bbbbbbbb-0", "version": 7, "publish_hash": "h7", "created_at": "2026-10-02T18:26:00.000Z", "bytes": 1200 },
            { "id": "0000000007-bbbbbbbb-1", "version": 7, "publish_hash": "h7", "created_at": "2026-10-02T18:40:00.000Z", "bytes": 300 },
            { "id": "../../admin", "version": 9 },
            { "version": 8 },
        ]);
        let items = change_logs_from(&body);
        let ids: Vec<&str> = items.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["0000000007-bbbbbbbb-1", "0000000007-bbbbbbbb-0", "0000000006-aaaaaaaa-0"]);
        assert_eq!(
            items[0],
            ChangeLogSummary {
                id: "0000000007-bbbbbbbb-1".into(),
                version: 7,
                publish_hash: "h7".into(),
                created_at: "2026-10-02T18:40:00.000Z".into(),
                bytes: 300,
            }
        );
        assert!(change_logs_from(&serde_json::json!({ "error": "Not found" })).is_empty());
    }

    #[test]
    fn only_an_id_the_api_gives_goes_into_a_url() {
        let uuid = "07d084d8-1c2b-4e5f-9a0b-123456789abc";
        assert_eq!(api_id(uuid), Some(uuid));
        assert_eq!(api_id("0000000007-bbbbbbbb-1"), Some("0000000007-bbbbbbbb-1"));
        let long = "x".repeat(129);
        for bad in ["", "../x", "a/b", "a b", "a?b=c", "a%2Fb", "a\\b", "é", long.as_str()] {
            assert_eq!(api_id(bad), None, "{bad:?} went into a URL");
        }
    }

    fn change_log_app() -> App {
        let mut app = App::new();
        app.add_message::<RefreshMyChangeLogs>()
            .add_message::<DownloadChangeLog>()
            .init_resource::<MyChangeLogs>()
            .add_systems(Update, refresh_my_change_logs);
        app
    }

    #[test]
    fn signed_out_change_logs_say_so_without_a_request() {
        let mut app = change_log_app();
        app.world_mut().write_message(RefreshMyChangeLogs { listing_id: "sim-1".into() });
        app.world_mut().write_message(DownloadChangeLog {
            listing_id: "sim-1".into(),
            log_id: "0000000007-bbbbbbbb-0".into(),
            save_to: "CHANGELOG-v7.md".into(),
        });
        app.update();
        let logs = app.world().resource::<MyChangeLogs>();
        assert_eq!(logs.listing.as_deref(), Some("sim-1"));
        assert_eq!(logs.state, MyListingsState::Error("Sign in to see this listing's change logs.".into()));
        assert!(logs.pending.is_none() && logs.saving.is_empty(), "a request went out signed out");
        assert_eq!(logs.downloads.get("0000000007-bbbbbbbb-0"), Some(&Err("Sign in to download this change log.".to_string())));
    }

    #[test]
    fn asking_about_another_listing_drops_the_answer_owed_for_the_last() {
        let mut app = change_log_app();
        let (tx, rx) = mpsc::channel();
        {
            let mut logs = app.world_mut().resource_mut::<MyChangeLogs>();
            logs.listing = Some("sim-1".into());
            logs.items = vec![ChangeLogSummary { id: "a".into(), version: 1, publish_hash: "h".into(), created_at: String::new(), bytes: 1 }];
            logs.downloads.insert("a".into(), Ok("CHANGELOG-v1.md".into()));
            logs.pending = Some(Mutex::new(rx));
            logs.state = MyListingsState::Loading;
        }
        app.world_mut().write_message(RefreshMyChangeLogs { listing_id: "../sim-2".into() });
        app.update();
        let logs = app.world().resource::<MyChangeLogs>();
        assert_eq!(logs.listing.as_deref(), Some("../sim-2"));
        assert!(logs.items.is_empty() && logs.downloads.is_empty() && logs.pending.is_none());
        assert_eq!(logs.state, MyListingsState::Error("That is not a listing the API gave.".into()));
        assert!(tx.send(Ok(Vec::new())).is_err(), "sim-1's answer could still arrive under sim-2");
    }

    #[test]
    fn a_finished_download_is_reported_by_its_log_id() {
        let mut app = change_log_app();
        let (tx, rx) = mpsc::channel();
        let (gone_tx, gone_rx) = mpsc::channel::<Result<std::path::PathBuf, String>>();
        {
            let mut logs = app.world_mut().resource_mut::<MyChangeLogs>();
            logs.saving.push(("log-1".into(), Mutex::new(rx)));
            logs.saving.push(("log-2".into(), Mutex::new(gone_rx)));
        }
        app.update();
        assert!(app.world().resource::<MyChangeLogs>().downloads.is_empty(), "a download still going has an entry");
        tx.send(Ok("CHANGELOG-v7.md".into())).unwrap();
        drop(gone_tx);
        app.update();
        let logs = app.world().resource::<MyChangeLogs>();
        assert_eq!(logs.downloads.get("log-1"), Some(&Ok(std::path::PathBuf::from("CHANGELOG-v7.md"))));
        assert_eq!(logs.downloads.get("log-2"), Some(&Err("The download stopped without an answer.".to_string())));
        assert!(logs.saving.is_empty());
    }
}
