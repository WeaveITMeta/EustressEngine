//! # Joining a host, and opening a world that arrived
//!
//! Launched with `--connect <host:port>` (or a `eustress-player://join/...` link),
//! the Player joins a Studio host over WebTransport, downloads the host's
//! Space as `.echk` chunks, opens it, and appears at the spawn point the host
//! chose. Everyone in the session then sees everyone else move.
//!
//! The same opening path serves a published simulation downloaded by
//! [`super::space_fetch`]: both arrive as a [`WorldDownloaded`], are written
//! into this process's world folder ([`super::live_world`]), opened by
//! [`super::space_world`], and only then get the local avatar: a few frames
//! after the Space's colliders exist, and once its terrain has landed
//! ([`SpaceTerrainLoad::ready`]), so the character stands on them.

use bevy::prelude::*;
use eustress_common::avatar::profile::SpawnSavedAvatar;
use eustress_networking::native;
use eustress_networking::session::{begin_join, ChunkCacheRes, LocalWorldReady, NetNotice, NetPlugin, WorldDownloaded};
use eustress_networking::join_link::JoinTicket;
use eustress_networking::JoinLink;

use super::live_world::{materialize, DiskChunkCache, LiveWorld};
use super::space_world::{OpenSpace, OpenSpaceRequest, SpaceOpened, SpaceTerrainLoad};
use super::join_notice::{self, PlainNotice};
use super::space_fetch::SpaceFetchFailed;
use eustress_play_runtime::screen_notice::{Notice, ScreenNotice};

/// Frames between a Space opening and its avatar spawning.
const SETTLE_FRAMES: u8 = 3;

/// Set by `main` when the Player was launched to join a host. Not `Clone`,
/// and its `Debug` never prints the identity: the ticket in it goes into one
/// `Hello` and nowhere else (`docs/networking/PLAYER_JOIN_TICKET.md`).
#[derive(Resource)]
pub struct JoinTarget {
    pub link: JoinLink,
    pub name: String,
    /// An identity ticket bound to the host's pin: how the host learns which
    /// account joined. The one a gallery's Play page put in the link, else
    /// one minted from this Player's sign-in. `start_join` takes it for the
    /// `Hello`, so it is `None` from then on.
    pub identity: Option<String>,
    /// Whether the join carries an identity, set when the target is made. It
    /// still says so after `start_join` has taken the ticket.
    pub signed_in: bool,
    /// Whether the identity is the gallery's ticket from the link. That
    /// ticket works once, so a failed join is joined again from the gallery,
    /// never retried.
    pub gallery_ticket: bool,
    /// Joining as a guest because Studio on this computer is signed in to
    /// another Eustress service only: `(signed in to, this one)`, as people
    /// call them. Set by `start_join`; a refused join says so.
    pub signed_in_elsewhere: Option<(String, String)>,
}

impl JoinTarget {
    /// The target for `link`. `ticket`: the gallery's, from the link
    /// (`LinkExtras::ticket`). Without one, `mint` makes one from this
    /// Player's sign-in (`commerce::join_ticket`); with one, nothing is minted.
    pub fn new(link: JoinLink, name: String, ticket: Option<JoinTicket>, mint: impl FnOnce(&JoinLink) -> Option<String>) -> Self {
        let gallery_ticket = ticket.is_some();
        let identity = match ticket {
            Some(ticket) => Some(ticket.into_secret()),
            None => mint(&link),
        };
        Self { link, name, signed_in: identity.is_some(), gallery_ticket, identity, signed_in_elsewhere: None }
    }
}

impl std::fmt::Debug for JoinTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoinTarget")
            .field("link", &self.link)
            .field("name", &self.name)
            .field("signed_in", &self.signed_in)
            .field("gallery_ticket", &self.gallery_ticket)
            .field("signed_in_elsewhere", &self.signed_in_elsewhere)
            .field("identity", &format_args!("{}", if self.identity.is_some() { "<set>" } else { "<none>" }))
            .finish()
    }
}

/// The avatar waiting for its world.
#[derive(Resource, Debug)]
struct PendingAvatar {
    at: Vec3,
    frames_since_open: Option<u8>,
    /// The Space that opened, once it has.
    root: Option<std::path::PathBuf>,
}

pub struct NetPlayPlugin;

impl Plugin for NetPlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((NetPlugin, super::net_replica::NetReplicaPlugin))
            // What the host's log says this Player runs.
            .insert_resource(eustress_networking::AppVersion(concat!("eustress-client ", env!("CARGO_PKG_VERSION")).into()))
            .add_systems(Startup, start_join.run_if(resource_exists::<JoinTarget>))
            .add_systems(Update, (open_arrived_world, spawn_avatar_in_opened_world, log_net_notices).chain())
            .add_systems(Update, show_fetch_failures);
    }
}

fn start_join(mut commands: Commands, mut target: ResMut<JoinTarget>, mut screen: ResMut<ScreenNotice>) {
    info!(
        "net: joining {}:{} as {}{}",
        target.link.host,
        target.link.port,
        target.name,
        if target.link.pin.is_some() { "" } else { " (unpinned: this machine only)" }
    );
    // A guest may be one only because Studio here is signed in to another
    // Eustress service, whose sign-in this Player never sends here.
    if !target.signed_in {
        target.signed_in_elsewhere = super::credential::signed_in_elsewhere();
    }
    // Taken, not copied: the ticket goes into this one `Hello` and nowhere
    // else, and a join that fails is never tried again with it.
    let identity = target.identity.take();
    match native::start_join(&target.link) {
        Ok(link) => {
            commands.insert_resource(ChunkCacheRes(Box::new(DiskChunkCache::default())));
            begin_join(&mut commands, link, &target.name, identity);
        }
        Err(e) => {
            error!("net: {e}");
            show(&mut screen, join_failure_notice(Some(&*target), &e));
        }
    }
}

fn open_arrived_world(
    mut commands: Commands,
    mut arrivals: MessageReader<WorldDownloaded>,
    live: Option<Res<LiveWorld>>,
    mut open: MessageWriter<OpenSpaceRequest>,
    mut screen: ResMut<ScreenNotice>,
) {
    for WorldDownloaded(world) in arrivals.read() {
        let Some(live) = live.as_deref() else {
            error!("net: a world arrived but this Player has no world folder (launch with --connect or --sim)");
            continue;
        };
        match materialize(world, live) {
            Ok(files) => {
                info!("net: opening {} from {} ({files} files)", world.start_space, world.universe);
                commands.insert_resource(PendingAvatar {
                    at: Vec3::from_array(world.spawn),
                    frames_since_open: None,
                    root: None,
                });
                open.write(OpenSpaceRequest { root: live.space_root.clone() });
            }
            Err(e) => {
                error!("net: could not open the world: {e}");
                show(&mut screen, join_notice::world_failed(&e));
            }
        }
    }
}

fn spawn_avatar_in_opened_world(
    mut commands: Commands,
    mut opened: MessageReader<SpaceOpened>,
    pending: Option<ResMut<PendingAvatar>>,
    terrain: Option<Res<SpaceTerrainLoad>>,
    mut spawn: MessageWriter<SpawnSavedAvatar>,
    mut ready: MessageWriter<LocalWorldReady>,
    open: Option<Res<OpenSpace>>,
    join: Option<Res<JoinTarget>>,
) {
    // Pending first: with no avatar waiting, SpaceOpened stays unread rather
    // than being consumed by a frame that could not use it.
    let Some(mut pending) = pending else { return };
    if let Some(space) = opened.read().last() {
        info!("net: opened {} ({} parts)", space.root.display(), space.parts);
        pending.frames_since_open = Some(0);
        pending.root = Some(space.root.clone());
    }
    let Some(frames) = pending.frames_since_open.as_mut() else { return };
    // Saturating: a large voxel terrain can take longer than 255 frames.
    *frames = frames.saturating_add(1);
    let terrain_ready = terrain.as_deref().map_or(true, SpaceTerrainLoad::ready);
    if *frames < SETTLE_FRAMES || !terrain_ready {
        return;
    }
    // The Space's `Players.CharacterAutoLoads`, by Studio's rule: off, nobody
    // gets a body, and a paddle game is played through its camera and input.
    if pending.root.as_deref().map_or(true, eustress_common::play_session::space_character_auto_loads) {
        // Sent only when the avatar service is on the origin that issued it.
        let token = super::credential::studio_token_for(&eustress_common::api_base::api_url(
            eustress_common::avatar::profile::PROFILE_PATH,
        ));
        // A host placed a joined player in metres, so its position stands. A
        // published world has no host: its place is the Space's own
        // SpawnLocation, read in the Space's units (`OpenSpace.spawn`). The
        // download's raw position is not converted, and would put the avatar
        // 3.3 to 3.6 times too far out (C1).
        let at = match (join.is_none(), open.as_deref().and_then(|o| o.spawn)) {
            (true, Some(place)) => place,
            _ => pending.at,
        };
        spawn.write(SpawnSavedAvatar { token, at });
    } else {
        // No avatar, so none is announced or sent, and the host builds no
        // replica: the Space's own camera is the view.
        info!("net: this Space gives players no avatar (Players.CharacterAutoLoads is off)");
    }
    // Tells a host we are in the world. With no session (a published
    // simulation played solo) nothing reads it.
    ready.write(LocalWorldReady);
    commands.remove_resource::<PendingAvatar>();
}

/// A failure on the screen as well as in the log, worded by [`join_notice`].
fn show(screen: &mut ScreenNotice, n: PlainNotice) {
    screen.show(Notice { title: n.title, detail: n.detail, hint: n.hint });
}

/// A failed join in plain words, for how this Player joined. One that sent a
/// gallery's ticket is joined again from the gallery, since that ticket works
/// once and nothing sends it again. Reads `signed_in`, not the identity,
/// which `start_join` has taken by then.
fn join_failure_notice(target: Option<&JoinTarget>, reason: &str) -> PlainNotice {
    match target {
        Some(t) if t.gallery_ticket => join_notice::join_failed_with_gallery_ticket(reason),
        Some(JoinTarget { signed_in: false, signed_in_elsewhere: Some((signed_in_to, this_one)), .. }) => {
            let mut n = join_notice::join_failed(reason, false);
            // Where a guest is told to sign in (a signed-in player would get
            // other help), say which service the sign-in is for.
            if n.hint != join_notice::join_failed(reason, true).hint {
                n.hint = Some(super::credential::signed_in_elsewhere_hint(signed_in_to, this_one));
            }
            n
        }
        _ => join_notice::join_failed(reason, target.is_some_and(|t| t.signed_in)),
    }
}

/// The session ending in plain words. After a gallery's ticket, the way back
/// in is the gallery too.
fn disconnect_notice(target: Option<&JoinTarget>, reason: &str) -> PlainNotice {
    match target {
        Some(t) if t.gallery_ticket => join_notice::disconnected_with_gallery_ticket(reason),
        _ => join_notice::disconnected(reason),
    }
}

/// A published simulation that would not download says why on screen.
fn show_fetch_failures(mut failures: MessageReader<SpaceFetchFailed>, mut screen: ResMut<ScreenNotice>) {
    if let Some(failure) = failures.read().last() {
        let signed_in = super::commerce::auth_token().is_some();
        let elsewhere = if signed_in || failure.gallery_ticket { None } else { super::credential::signed_in_elsewhere() };
        show(&mut screen, fetch_failure_notice(&failure.error, failure.gallery_ticket, signed_in, elsewhere.as_ref()));
    }
}

/// What a solo download that failed shows. A launch with the gallery's
/// one-time ticket goes back to the gallery. A guest only because Studio here
/// is signed in to another Eustress service (`elsewhere`: signed in to, this
/// one) is told which service to sign in to, where a guest would be told to
/// sign in.
fn fetch_failure_notice(error: &str, gallery_ticket: bool, signed_in: bool, elsewhere: Option<&(String, String)>) -> PlainNotice {
    if gallery_ticket {
        return join_notice::fetch_failed_with_gallery_ticket(error);
    }
    let mut n = join_notice::fetch_failed(error, signed_in);
    if let (false, Some((signed_in_to, this_one))) = (signed_in, elsewhere) {
        if n.hint != join_notice::fetch_failed(error, true).hint {
            n.hint = Some(super::credential::signed_in_elsewhere_hint(signed_in_to, this_one));
        }
    }
    n
}

fn log_net_notices(
    mut notices: MessageReader<NetNotice>,
    mut last_percent: Local<u64>,
    target: Option<Res<JoinTarget>>,
    mut screen: ResMut<ScreenNotice>,
) {
    // How this Player joined: with an account, which a host that turns away
    // guests needs, and whether with a gallery's single-use ticket.
    let target = target.as_deref();
    for notice in notices.read() {
        match notice {
            NetNotice::Joined { peer, host_name } => info!("net: joined {host_name}'s session as player {peer}"),
            NetNotice::JoinFailed { reason } => {
                error!("net: could not join: {reason}");
                show(&mut screen, join_failure_notice(target, reason));
            }
            NetNotice::Disconnected { reason } => {
                warn!("net: disconnected: {reason}");
                show(&mut screen, disconnect_notice(target, reason));
            }
            NetNotice::Downloading { received, total } => {
                let percent = if *total == 0 { 100 } else { received.saturating_mul(100) / total };
                if percent >= *last_percent + 10 || percent == 100 {
                    *last_percent = percent;
                    info!("net: downloading the world: {percent}% of {} KB", total / 1024);
                }
            }
            NetNotice::PeerJoined { name, .. } => info!("net: {name} is here"),
            NetNotice::PeerLeft { name, .. } => info!("net: {name} left"),
            NetNotice::Chat { name, text, .. } => info!("[{name}] {text}"),
            // Answered by the commerce module, which asks the player.
            NetNotice::PurchasePrompt { prompt, product, .. } => info!("net: the host offers product {product} (prompt {prompt})"),
            NetNotice::Hosting { .. }
            | NetNotice::HostEnded { .. }
            | NetNotice::Admit { .. }
            | NetNotice::PeerIdentity { .. }
            | NetNotice::PurchaseClosed { .. }
            | NetNotice::Receipts { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown_after(notice: NetNotice, target: Option<JoinTarget>) -> Notice {
        let mut app = App::new();
        app.add_message::<NetNotice>().init_resource::<ScreenNotice>().add_systems(Update, log_net_notices);
        if let Some(target) = target {
            app.insert_resource(target);
        }
        app.world_mut().write_message(notice);
        app.update();
        app.world().resource::<ScreenNotice>().0.clone().expect("a notice on the screen")
    }

    fn target(identity: Option<&str>) -> JoinTarget {
        let link = JoinLink::parse("eustress://join/127.0.0.1:7777").expect("a link");
        JoinTarget::new(link, "Ada".into(), None, |_| identity.map(str::to_string))
    }

    /// A ticket's shape. No real ticket is ever written into a test.
    const SECRET: &str = "eit1.c2VjcmV0LXRpY2tldA.c2ln-_x";

    /// The target a gallery's join link makes, with the ticket from the link.
    fn gallery_target() -> JoinTarget {
        let link = format!(
            "eustress-player://join/203.0.113.7:7777?key=abcdef&pin={}&ticket={SECRET}&api=staging",
            "ab".repeat(32)
        );
        let (rest, mut extras) = eustress_networking::join_link::split_link_extras(&link).expect("a gallery link");
        let link = JoinLink::parse(&rest).expect("a link");
        JoinTarget::new(link, "Ada".into(), extras.ticket.take(), |_| panic!("a join with the gallery's ticket mints none"))
    }

    /// What `start_join` leaves: the identity sent, and gone.
    fn sent(mut target: JoinTarget) -> Option<JoinTarget> {
        target.identity.take();
        Some(target)
    }

    /// The gallery's ticket from the link is the join's identity, nothing is
    /// minted in its place, and the target's `Debug` never prints it, before
    /// or after `start_join` takes it.
    #[test]
    fn a_gallery_ticket_is_the_identity_and_never_printed() {
        let mut target = gallery_target();
        assert_eq!(target.identity.as_deref(), Some(SECRET));
        assert!(target.signed_in && target.gallery_ticket);
        let shown = format!("{target:?}");
        assert!(!shown.contains(SECRET), "the target's Debug printed its ticket");
        assert!(shown.contains("identity: <set>"), "the target's Debug hid that it has an identity");
        assert_eq!(target.identity.take().as_deref(), Some(SECRET), "what start_join sends");
        let shown = format!("{target:?}");
        assert!(shown.contains("identity: <none>") && shown.contains("signed_in: true"), "sent, and still signed in");
    }

    /// Without a ticket in the link, a signed-in Player mints its own, as
    /// before, and a guest has none.
    #[test]
    fn without_a_gallery_ticket_the_player_mints_its_own() {
        let link = JoinLink::parse("eustress://join/203.0.113.7:7777").expect("a link");
        let member = JoinTarget::new(link.clone(), "Ada".into(), None, |l| (l.port == 7777).then(|| "minted".to_string()));
        assert_eq!((member.identity.as_deref(), member.signed_in, member.gallery_ticket), (Some("minted"), true, false));
        let guest = JoinTarget::new(link, "Ada".into(), None, |_| None);
        assert_eq!((guest.identity.as_deref(), guest.signed_in, guest.gallery_ticket), (None, false, false));
    }

    /// The notice reads `signed_in`, not the identity `start_join` has taken
    /// by the time a join fails: a signed-in player is not told to sign in,
    /// and one that came with a gallery's ticket is sent back to the gallery,
    /// never to the same link.
    #[test]
    fn a_failed_join_is_worded_for_how_the_player_joined() {
        let refused = || NetNotice::JoinFailed { reason: "the host refused the join: This ticket was already used.".into() };
        let gallery = shown_after(refused(), sent(gallery_target()));
        assert_eq!(gallery.detail, "This ticket was already used.");
        let hint = gallery.hint.unwrap_or_default();
        assert!(hint.contains("Join again from the gallery") && !hint.contains("Sign in"), "{hint}");
        let member = shown_after(refused(), sent(target(Some("minted"))));
        assert!(!member.hint.unwrap_or_default().contains("Sign in"), "a signed-in player is not told to sign in");
        let ended = shown_after(
            NetNotice::Disconnected { reason: "the host closed the connection: stopped".into() },
            sent(gallery_target()),
        );
        assert!(ended.hint.unwrap_or_default().contains("Join again from the gallery"));
    }

    /// A join the host refused is on the screen in the host's words, and a
    /// Player that joined as a guest is told to sign in.
    #[test]
    fn a_refused_join_is_shown_on_screen() {
        let refused = || NetNotice::JoinFailed { reason: "the host refused the join: The gallery did not admit this join.".into() };
        let guest = shown_after(refused(), Some(target(None)));
        assert_eq!(guest.title, "The host did not let you in");
        assert_eq!(guest.detail, "The gallery did not admit this join.");
        assert!(guest.hint.unwrap().contains("Sign in"));
        let member = shown_after(refused(), Some(target(Some("ticket"))));
        assert!(!member.hint.unwrap_or_default().contains("Sign in"), "a signed-in player is not told to sign in");
    }

    /// A guest because Studio here is signed in to production only, refused
    /// by a staging host, is told which service to sign in to; a failure
    /// that has nothing to do with signing in keeps its own help.
    /// A solo download refused for a guest who is one only because Studio
    /// here is signed in to the other service says which service to sign in
    /// to; a launch with the gallery's ticket goes back to the gallery.
    #[test]
    fn a_solo_fetch_failure_names_the_other_service_or_the_gallery() {
        let rated = r#"403 Forbidden from https://api-staging.eustress.dev/api/simulations/07d0/world/manifest: {"code":"sign_in_required"}"#;
        let elsewhere = ("production".to_string(), "staging".to_string());
        let n = fetch_failure_notice(rated, false, false, Some(&elsewhere));
        assert_eq!(n.title, "This simulation is rated for older players");
        assert_eq!(n.hint, Some(super::super::credential::signed_in_elsewhere_hint("production", "staging")));
        assert_eq!(fetch_failure_notice(rated, false, false, None), join_notice::fetch_failed(rated, false), "signed in nowhere");
        let missing = "404 Not Found from https://api-staging.eustress.dev/api/simulations/07d0/world/manifest: ";
        assert_eq!(fetch_failure_notice(missing, false, false, Some(&elsewhere)), join_notice::fetch_failed(missing, false), "not about signing in");
        let used = format!("{}: 403 used", join_notice::PLAY_TICKET_REFUSED);
        let n = fetch_failure_notice(&used, true, false, Some(&elsewhere));
        assert_eq!((n.title.as_str(), n.hint.as_deref()), ("This Play link cannot be used", Some("Join again from the gallery.")));
    }

    #[test]
    fn player_token_origin_join_notice_names_the_other_service() {
        let elsewhere = || {
            let mut t = target(None);
            t.signed_in_elsewhere = Some(("production".into(), "staging".into()));
            Some(t)
        };
        let refused = || NetNotice::JoinFailed { reason: "the host refused the join: The gallery did not admit this join.".into() };
        let shown = shown_after(refused(), elsewhere());
        assert_eq!(shown.detail, "The gallery did not admit this join.");
        assert_eq!(
            shown.hint.as_deref(),
            Some(
                "This Player is signed in to production but this simulation is on staging. \
                 Sign in to staging with Eustress Studio, then press Play again."
            )
        );
        let unreachable = || NetNotice::JoinFailed { reason: "could not reach the host at 203.0.113.7:7777: timed out".into() };
        assert_eq!(shown_after(unreachable(), elsewhere()).hint, shown_after(unreachable(), Some(target(None))).hint);
        let plain_guest = shown_after(refused(), Some(target(None)));
        assert!(!plain_guest.hint.unwrap_or_default().contains("production"), "a guest signed in nowhere");
    }

    /// Losing the session after joining says so too.
    #[test]
    fn a_disconnect_is_shown_on_screen() {
        let shown = shown_after(NetNotice::Disconnected { reason: "the host closed the connection: stopped".into() }, None);
        assert_eq!(shown.title, "The session ended");
    }
}
