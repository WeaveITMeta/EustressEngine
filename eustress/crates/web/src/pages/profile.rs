// =============================================================================
// Eustress Web - The profile page
// =============================================================================
// Who this person is, what they made, and what they are up to, in the Gallery's look: glass panels, chips, mono numbers and the
// Gallery's own cards. On someone else's profile, Follow and Unfollow. On your own profile, and nowhere else, Edit profile and
// your account beside it: balances, Purchases, Cases, Settings, your avatar and your spending.
//
// `/profile` (your own) and `/profile/:username` (anyone's). Nothing on this page is invented: the old page's Friends and
// Inventory tabs, its stand-in stats and its Follow button that changed nothing are gone (McKale, 2026-10-09), and a test
// holds that.
//
// The Worker decides what a profile may say (src/account_profile.mjs, under Security review): invisible characters are
// removed, a display name may not pass for staff or for another account, edits are limited to 10 an hour, and the picture
// is a path under /assets only. A new display name or bio waits for review: visitors keep seeing the approved value (or
// the username, or no bio), and the owner alone sees what waits, marked "Pending review" (the Worker sends it to the owner
// only, and this page shows it on your own profile only). Wherever a display name is shown, the @username is shown beside it (here, and the friend
// cards in pages/friends.rs, through handle_of), so a chosen name never stands alone.
//
// Every number and picture is something the Worker said, and every change goes to the Worker:
//
//   GET  /api/community/users/{username}   who: username, display_name, avatar_url, join_date, bio, badges, is_verified,
//                                          follower_count, following_count, total_plays, is_following (for the viewer)
//   GET  /api/simulations                  what they made: the listed Spaces whose author_name is theirs, as Gallery cards. The
//                                          Worker has no per-author list, and this one reads at most 100 listing records.
//   GET  /api/simulations/live             what they are up to: the live sessions of those Spaces
//   POST /api/social/follow, /unfollow     { username }: Follow and Unfollow; the Followers figure moves with the answer
//   POST /api/account/profile              { display_name, bio, avatar_url }: Edit profile (src/account_profile.mjs)
//   the session and the top bar's Delta    your own profile only: Tickets, Bliss and Delta
//   GET  /api/purchases/summary            your own profile only: what you spent (SpendSummaryCard)
//   GET, PUT /api/avatar                   your own profile only: your avatar, in the avatar customizer
//
// The profile route also sends friend_count, favorite_count, inventory_count and simulation_count. This page reads none of
// them: the first three describe a person's private activity (McKale: hidden), and simulation_count counts Spaces that are
// not in the Gallery. The Verified mark comes from `is_verified` alone; the Worker's badge list repeats it as a badge, which is
// left out so it is never shown twice or shown on its own.
//
// Table of Contents:
// 1. What the Worker says, and the pure rules (tested)
// 2. The page
// 3. The profile
// 4. Follow, and Edit profile
// 5. Your account (your own profile only)
// 6. Loading, signed out, missing, failed
// 7. Tests
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_params_map;
use serde::{Deserialize, Serialize};

use crate::api::catalog::{self, Listing, LiveNow, Session};
use crate::api::{self, get_purchase_summary, ApiClient, ApiError, PurchaseSummary};
use crate::components::avatar_customizer::AvatarCustomizer;
use crate::components::{CentralNav, Footer};
use crate::pages::gallery::{ListingCard, LiveCard};
use crate::state::{AppState, AuthState, DeltaState};
use crate::utils::{format_bliss_exact, format_count};

// -----------------------------------------------------------------------------
// 1. What the Worker says, and the pure rules
// -----------------------------------------------------------------------------

/// `GET /api/community/users/{username}`, the fields this page reads (handleUserProfile in the Worker).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ProfileReply {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    /// "October 2026", as the Worker words it.
    #[serde(default)]
    pub join_date: String,
    #[serde(default)]
    pub follower_count: u64,
    #[serde(default)]
    pub following_count: u64,
    /// Visits to every Space they made.
    #[serde(default)]
    pub total_plays: u64,
    #[serde(default)]
    pub badges: Vec<BadgeReply>,
    /// True only when the account's identity was verified (`kyc_verified` on the account record).
    #[serde(default)]
    pub is_verified: bool,
    /// The signed-in viewer follows this account. False for a signed-out viewer.
    #[serde(default)]
    pub is_following: bool,
    /// What waits for review. The Worker sends it to the owner alone; this page shows it on your own profile alone.
    #[serde(default)]
    pub pending: PendingFields,
    /// Why each waits: a person will look, the check could not run (with its sentence), or there is no check yet.
    #[serde(default)]
    pub pending_notes: PendingNotes,
}

/// The Worker's note on one waiting field: `state` is 'review', 'unavailable' or 'waiting'; `reason` is the check's own
/// sentence for the owner, sent with 'unavailable' when the check gave one.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct PendingNote {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct PendingNotes {
    #[serde(default)]
    pub display_name: Option<PendingNote>,
    #[serde(default)]
    pub bio: Option<PendingNote>,
}

/// What the owner is told under a waiting value.
pub fn note_words(note: Option<&PendingNote>) -> String {
    match note {
        Some(n) if n.state == "review" => "A person will look at this before anyone else sees it.".to_string(),
        Some(n) if n.state == "unavailable" => n
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "The check could not run. It runs again when you next save.".to_string()),
        _ => "Waiting for the check before anyone else sees it.".to_string(),
    }
}

/// A display name or bio saved and waiting for review.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct PendingFields {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
}

impl PendingFields {
    fn clean(text: &Option<String>) -> Option<String> {
        text.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string)
    }
    pub fn display_name(&self) -> Option<String> {
        Self::clean(&self.display_name)
    }
    pub fn bio(&self) -> Option<String> {
        Self::clean(&self.bio)
    }
}

/// One badge the Worker computed (computeBadges). Its emoji icon is not used; the site draws its own chips.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct BadgeReply {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// The Spaces in the Gallery by this person.
#[derive(Clone, Debug, PartialEq)]
pub enum Shelf {
    Loading,
    Ready(Vec<Listing>),
    Failed,
}

/// The Delta figure in your account panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeltaShown {
    Waiting,
    Unavailable,
    Whole(u64),
}

/// Your own account, as the session has it. It exists only when the profile on screen is yours.
#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub tickets: u64,
    pub bliss: f64,
    pub delta: DeltaShown,
}

/// Whose profile to show: the one the address names, or the signed-in account's own at `/profile`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Named(String),
    /// `/profile` while the session is still being read.
    Waiting,
    /// `/profile` signed out.
    SignedOut,
}

pub fn target_of(param: Option<&str>, auth: &AuthState) -> Target {
    if let Some(name) = param.map(str::trim).filter(|n| !n.is_empty()) {
        return Target::Named(name.to_string());
    }
    match auth {
        AuthState::Authenticated(user) => Target::Named(user.username.clone()),
        AuthState::Unknown => Target::Waiting,
        AuthState::Unauthenticated => Target::SignedOut,
    }
}

/// The profile on screen is the viewer's own: the signed-in username and the Worker's username are the same, exactly.
pub fn is_own(viewer: Option<&str>, profile_username: &str) -> bool {
    !profile_username.is_empty() && viewer.is_some_and(|v| v == profile_username)
}

/// Your account, only when the profile is yours. On anyone else's profile there is nothing to show, so nothing is built.
pub fn account_of(auth: &AuthState, delta: &DeltaState, profile_username: &str) -> Option<Account> {
    let user = auth.user()?;
    if !is_own(Some(&user.username), profile_username) {
        return None;
    }
    let delta = match delta {
        DeltaState::Balance(b) => DeltaShown::Whole(b.whole()),
        DeltaState::Absent => DeltaShown::Unavailable,
        DeltaState::Unknown | DeltaState::Loading => DeltaShown::Waiting,
    };
    Some(Account { tickets: user.ticket_balance, bliss: user.bliss_balance, delta })
}

/// The name in the heading: the display name, or the username when there is none.
pub fn name_of(profile: &ProfileReply) -> String {
    profile.display_name.as_deref().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&profile.username).to_string()
}

/// The badges shown as chips. The Verified mark is drawn from `is_verified` alone, so a "verified" badge is never a chip.
pub fn shown_badges(profile: &ProfileReply) -> Vec<BadgeReply> {
    profile.badges.iter().filter(|b| b.id != "verified" && !b.name.trim().is_empty()).cloned().collect()
}

/// The longest avatar address the Worker keeps, in bytes.
pub const AVATAR_MAX_BYTES: usize = 512;
/// The longest display name and bio the Worker keeps, in characters once whitespace is folded.
pub const DISPLAY_NAME_MAX: usize = 32;
pub const BIO_MAX: usize = 280;

/// A picture address this site shows, by the Worker's own rule (account_profile.mjs cleanAvatarUrl): a path under /assets of
/// letters, digits and `/ _ . -`, with no `..` or `//`. No address on any host, ours included: there is no picture of an
/// account's own that the Worker has approved by id. Anything else draws the monogram.
pub fn allowed_avatar(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() || url.len() > AVATAR_MAX_BYTES {
        return None;
    }
    let rest = url.strip_prefix("/assets/")?;
    let ok = !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || "/_.-".contains(c)) && !url.contains("..") && !url.contains("//");
    ok.then(|| url.to_string())
}

/// "@ada": the username, shown beside every display name so a chosen name never stands alone.
pub fn handle_of(username: &str) -> String {
    format!("@{username}")
}

/// An avatar address that may go into an `<img>`. Anything else draws the monogram.
pub fn safe_avatar(url: Option<&str>) -> Option<String> {
    allowed_avatar(url?)
}

/// Whitespace folded to single spaces and trimmed, as the Worker cleans display text before it measures it.
pub fn folded(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What is wrong with an edit before it is sent, by the Worker's limits: the field and a sentence. `None` when it may go.
pub fn edit_problem(display_name: &str, bio: &str, avatar_url: &str) -> Option<(&'static str, String)> {
    if folded(display_name).chars().count() > DISPLAY_NAME_MAX {
        return Some(("display_name", format!("Your display name can be at most {DISPLAY_NAME_MAX} characters.")));
    }
    if folded(bio).chars().count() > BIO_MAX {
        return Some(("bio", format!("Your bio can be at most {BIO_MAX} characters.")));
    }
    if !avatar_url.trim().is_empty() && allowed_avatar(avatar_url).is_none() {
        return Some(("avatar_url", "The picture must be a path under /assets.".to_string()));
    }
    None
}

/// Who is looking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Viewer {
    /// The session is still being read.
    Unknown,
    SignedOut,
    SignedIn,
}

pub fn viewer_of(auth: &AuthState) -> Viewer {
    match auth {
        AuthState::Unknown => Viewer::Unknown,
        AuthState::Unauthenticated => Viewer::SignedOut,
        AuthState::Authenticated(_) => Viewer::SignedIn,
    }
}

/// What the Follow control is on a profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowShown {
    /// Your own profile, or the session is still being read.
    Hidden,
    /// Signed out: a link to sign in.
    SignIn,
    Follow,
    Following,
}

pub fn follow_shown(viewer: &Viewer, own: bool, following: bool) -> FollowShown {
    match viewer {
        _ if own => FollowShown::Hidden,
        Viewer::Unknown => FollowShown::Hidden,
        Viewer::SignedOut => FollowShown::SignIn,
        Viewer::SignedIn if following => FollowShown::Following,
        Viewer::SignedIn => FollowShown::Follow,
    }
}

/// The Worker's route for following (`true`) or unfollowing (`false`).
pub fn follow_path(follow: bool) -> &'static str {
    if follow { "/api/social/follow" } else { "/api/social/unfollow" }
}

/// The Followers figure once the Worker said yes: one more for a new follow, one less for an unfollow, the same when the
/// viewer's state did not change (the Worker's routes are idempotent).
pub fn followers_after(followers: u64, was_following: bool, now_following: bool) -> u64 {
    match (was_following, now_following) {
        (false, true) => followers + 1,
        (true, false) => followers.saturating_sub(1),
        _ => followers,
    }
}

/// `POST /api/social/follow` and `/unfollow`.
#[derive(Debug, Serialize)]
pub struct FollowRequest<'a> {
    pub username: &'a str,
}

/// `POST /api/account/profile`: every field, so what the form shows is what is saved. An empty field clears it.
#[derive(Debug, Serialize, PartialEq)]
pub struct ProfileEdit {
    pub display_name: String,
    pub bio: String,
    pub avatar_url: String,
}

/// The Worker's answer: the profile as visitors now see it, and what waits for review.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct EditAnswer {
    #[serde(default)]
    pub profile: SavedProfile,
    #[serde(default)]
    pub pending: PendingFields,
    #[serde(default)]
    pub pending_notes: PendingNotes,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct SavedProfile {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub bio: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

/// The shown profile once an edit is saved: the Worker's approved values (the username when the display name is none), and
/// what waits for review.
pub fn with_saved(profile: &ProfileReply, saved: &EditAnswer) -> ProfileReply {
    let mut next = profile.clone();
    next.display_name = Some(saved.profile.display_name.clone()).filter(|n| !n.trim().is_empty());
    next.bio = Some(saved.profile.bio.clone());
    next.avatar_url = saved.profile.avatar_url.clone().filter(|u| !u.is_empty());
    next.pending = saved.pending.clone();
    next.pending_notes = saved.pending_notes.clone();
    next
}

/// A refusal from review still says what the profile now is (the refused value no longer waits).
pub fn answer_in_refusal(error: &ApiError) -> Option<EditAnswer> {
    match error {
        ApiError::Server { status: 400, message } => {
            let v: serde_json::Value = serde_json::from_str(message).ok()?;
            if v.get("code")?.as_str()? != "refused" {
                return None;
            }
            serde_json::from_value(v).ok()
        }
        _ => None,
    }
}

/// The sentence for a failed save. A Worker without the route answers 404.
pub fn save_failure(error: &ApiError) -> String {
    match error {
        ApiError::NotFound => "Editing a profile is not available on this server yet.".to_string(),
        other => api::failure_sentence(other),
    }
}

/// The letter the avatar shows when there is no picture.
pub fn monogram(name: &str) -> String {
    name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".to_string())
}

pub fn joined_words(join_date: &str) -> Option<String> {
    let d = join_date.trim();
    (!d.is_empty()).then(|| format!("Joined {d}"))
}

pub fn spaces_words(n: usize) -> String {
    if n == 1 { "1 Space".to_string() } else { format!("{} Spaces", format_count(n as u64)) }
}

/// The listed Spaces this person made, in the Worker's order (newest first).
pub fn made_by(listings: &[Listing], username: &str) -> Vec<Listing> {
    listings.iter().filter(|l| l.author_name.as_deref().is_some_and(|a| !a.trim().is_empty() && a.trim().eq_ignore_ascii_case(username.trim()))).cloned().collect()
}

/// The live sessions of this person's Spaces.
pub fn live_of(live: &LiveNow, username: &str) -> Vec<Session> {
    live.sessions.iter().filter(|s| s.author_name.as_deref().is_some_and(|a| !a.trim().is_empty() && a.trim().eq_ignore_ascii_case(username.trim()))).cloned().collect()
}

/// The figures beside the name. The Spaces count waits for the list, and is left out when the list could not load.
pub fn figures_of(profile: &ProfileReply, shelf: &Shelf) -> Vec<(&'static str, String)> {
    let spaces = match shelf {
        Shelf::Loading => Some("\u{2026}".to_string()),
        Shelf::Ready(list) => Some(format_count(list.len() as u64)),
        Shelf::Failed => None,
    };
    [
        spaces.map(|s| ("Spaces", s)),
        Some(("Visits", format_count(profile.total_plays))),
        Some(("Followers", format_count(profile.follower_count))),
        Some(("Following", format_count(profile.following_count))),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// What the Spaces section says when nothing of theirs is in the Gallery.
pub fn nothing_made(own: bool, username: &str) -> (String, &'static str) {
    if own {
        ("You have nothing in the Gallery yet".to_string(), "Build a Space in Eustress Studio and publish it. It shows here once it is approved.")
    } else {
        (format!("@{username} has nothing in the Gallery yet"), "Spaces they publish show here once they are approved.")
    }
}

/// What the live section says when none of their Spaces is being hosted.
pub fn not_live(own: bool, username: &str) -> (&'static str, String) {
    if own {
        ("You are not live right now", "When one of your Spaces is being hosted, it shows here with a Join button.".to_string())
    } else {
        ("Not live right now", format!("When one of @{username}'s Spaces is being hosted, it shows here with a Join button."))
    }
}

pub fn delta_words(delta: DeltaShown) -> String {
    match delta {
        DeltaShown::Waiting => "\u{2026}".to_string(),
        DeltaShown::Unavailable => "Unavailable".to_string(),
        DeltaShown::Whole(n) => format_count(n),
    }
}

// -----------------------------------------------------------------------------
// 2. The page
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Load {
    Waiting,
    SignedOut,
    Missing(String),
    Failed(String),
    Ready(ProfileReply),
}

fn load_of(answer: Result<ProfileReply, ApiError>, asked: &str) -> Load {
    match answer {
        Ok(mut profile) => {
            if profile.username.trim().is_empty() {
                profile.username = asked.to_string();
            }
            Load::Ready(profile)
        }
        Err(ApiError::NotFound) | Err(ApiError::Server { status: 400, .. }) => Load::Missing(asked.chars().take(40).collect()),
        Err(e) => Load::Failed(api::failure_sentence(&e)),
    }
}

#[component]
pub fn ProfilePage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let auth = app_state.auth;
    let delta = app_state.delta;
    let params = use_params_map();
    let target = Memo::new(move |_| target_of(params.read().get("username").as_deref(), &auth.get()));

    let load = RwSignal::new(Load::Waiting);
    let made = RwSignal::new(Shelf::Loading);
    let live = RwSignal::new(LiveNow::default());
    let attempt = RwSignal::new(0u32);
    let runs = StoredValue::new(0u64);

    // In the browser only: the profile, then the Gallery's list and its live sessions. A newer target wins over a slower answer.
    Effect::new(move |_| {
        let target = target.get();
        attempt.track();
        let run = runs.get_value() + 1;
        runs.set_value(run);
        made.set(Shelf::Loading);
        live.set(LiveNow::default());
        let name = match target {
            Target::Waiting => return load.set(Load::Waiting),
            Target::SignedOut => return load.set(Load::SignedOut),
            Target::Named(name) => name,
        };
        load.set(Load::Waiting);
        spawn_local(async move {
            let client = ApiClient::new(api_url.get_value());
            let path = format!("/api/community/users/{}", urlencoding::encode(&name));
            let answer = load_of(client.get::<ProfileReply>(&path).await, &name);
            if runs.try_get_value() != Some(run) {
                return;
            }
            let username = match &answer {
                Load::Ready(profile) => Some(profile.username.clone()),
                _ => None,
            };
            load.set(answer);
            let Some(username) = username else { return };
            let (list, now) = futures::join!(catalog::fetch_listings(&client), catalog::fetch_live_now(&client));
            if runs.try_get_value() != Some(run) {
                return;
            }
            made.set(match list {
                Ok(list) => Shelf::Ready(made_by(&list, &username)),
                Err(_) => Shelf::Failed,
            });
            live.set(now);
        });
    });

    view! {
        <div class="page g-page pn-page">
            <CentralNav active="".to_string() />
            <div class="g-backdrop" aria-hidden="true"></div>
            <main class="g-main">
                <div class="g-wrap pn-wrap">
                    {move || match load.get() {
                        Load::Waiting => view! { <ProfileSkeleton /> }.into_any(),
                        Load::SignedOut => view! { <ProfileSignedOut /> }.into_any(),
                        Load::Missing(name) => view! { <ProfileMissing name=name /> }.into_any(),
                        Load::Failed(why) => view! { <ProfileFailed why=why retry=Callback::new(move |_| attempt.update(|n| *n += 1)) /> }.into_any(),
                        Load::Ready(profile) => {
                            let username = profile.username.clone();
                            let account = Signal::derive(move || account_of(&auth.get(), &delta.get(), &username));
                            let viewer = Signal::derive(move || viewer_of(&auth.get()));
                            view! { <ProfileBody profile=profile account=account viewer=viewer made=made live=live /> }.into_any()
                        }
                    }}
                </div>
            </main>
            <Footer />
        </div>
    }
}

// -----------------------------------------------------------------------------
// 3. The profile
// -----------------------------------------------------------------------------

const CHECK_ICON: &str = "M5 12.5l4.2 4.2L19 7.5";
const LOCK_ICON: &str = "M7 10.5V8a5 5 0 0 1 10 0v2.5M5.5 10.5h13v10h-13z";

/// The avatar picture, or the monogram when the profile has no safe picture or the browser could not load the one it has. failed
/// holds the address that failed, not a flag, so a new picture (an edit saved from the panel) gets its own try.
fn avatar_face(profile: &ProfileReply, failed: RwSignal<Option<String>>) -> AnyView {
    let picture = safe_avatar(profile.avatar_url.as_deref()).filter(|src| failed.with(|bad| bad.as_deref() != Some(src.as_str())));
    match picture {
        Some(src) => {
            let url = src.clone();
            view! { <img src=src alt="" width="128" height="128" decoding="async" on:error=move |_| failed.set(Some(url.clone())) /> }.into_any()
        }
        None => view! { <span class="pn-monogram" aria-hidden="true">{monogram(&name_of(profile))}</span> }.into_any(),
    }
}

/// The profile itself. `account` is present only on your own profile; everything private is inside the panel it opens, and
/// Edit profile is there too. `viewer` decides the Follow control on anyone else's profile.
#[component]
pub fn ProfileBody(profile: ProfileReply, account: Signal<Option<Account>>, viewer: Signal<Viewer>, made: RwSignal<Shelf>, live: RwSignal<LiveNow>) -> impl IntoView {
    let username = profile.username.clone();
    let badges = shown_badges(&profile);
    let verified = profile.is_verified;
    let joined = joined_words(&profile.join_date);
    // What is shown moves with a follow and with a saved edit.
    let shown = RwSignal::new(profile);
    let editing = RwSignal::new(false);
    let own = Memo::new(move |_| account.with(|a| a.is_some()));
    let live_name = username.clone();
    let made_name = username.clone();
    let follow_name = username.clone();
    let customizing = RwSignal::new(false);
    // The avatar address the browser could not load: the monogram stands in for it.
    let avatar_failed = RwSignal::new(None::<String>);

    view! {
        <header class="g-glass pn-head" aria-labelledby="pn-name">
            <div class="pn-avatar">
                {move || shown.with(|p| avatar_face(p, avatar_failed))}
            </div>
            <div class="pn-id">
                <div class="pn-name-row">
                    <h1 id="pn-name" class="pn-name">{move || shown.with(name_of)}</h1>
                    {verified.then(|| view! {
                        <span class="g-chip pn-verified" title="Identity verified">
                            <svg viewBox="0 0 24 24" aria-hidden="true"><path d=CHECK_ICON /></svg>
                            "Verified"
                        </span>
                    })}
                </div>
                <div class="pn-handle">{handle_of(&username)}</div>
                {move || own.get().then(|| shown.with(|p| p.pending.display_name().map(|n| (n, note_words(p.pending_notes.display_name.as_ref()))))).flatten().map(|(name, note)| view! {
                    <div class="pn-pending"><span class="g-chip warn">"Pending review"</span><span class="pn-pending-text">{name}</span><small class="pn-pending-note">{note}</small></div>
                })}
                <div class="pn-meta">
                    {joined.map(|j| view! { <span>{j}</span> })}
                    {move || own.get().then(|| view! { <span class="g-chip">"This is you"</span> })}
                </div>
                {move || {
                    let bio = shown.with(|p| p.bio.as_deref().map(str::trim).unwrap_or_default().to_string());
                    (!bio.is_empty()).then(|| view! { <p class="pn-bio">{bio}</p> })
                }}
                {move || own.get().then(|| shown.with(|p| p.pending.bio().map(|b| (b, note_words(p.pending_notes.bio.as_ref()))))).flatten().map(|(bio, note)| view! {
                    <div class="pn-pending"><span class="g-chip warn">"Pending review"</span><p class="pn-bio pn-pending-text">{bio}</p><small class="pn-pending-note">{note}</small></div>
                })}
                <ul class="pn-badges" aria-label="Badges">
                    {if badges.is_empty() {
                        view! { <li class="g-hint">"No badges yet"</li> }.into_any()
                    } else {
                        badges.into_iter().map(|b| view! { <li><span class="g-chip pn-badge" title=b.description>{b.name}</span></li> }).collect_view().into_any()
                    }}
                </ul>
                <div class="pn-actions">
                    <FollowControl username=follow_name shown=shown viewer=viewer own=own />
                    <Show when=move || own.get() && !editing.get()>
                        <button type="button" class="g-btn pn-edit-open" on:click=move |_| editing.set(true)>"Edit profile"</button>
                    </Show>
                </div>
            </div>
            <dl class="pn-figures">
                {move || {
                    let figures = made.with(|shelf| shown.with(|p| figures_of(p, shelf)));
                    figures.into_iter().map(|(k, v)| view! { <div><dt>{k}</dt><dd>{v}</dd></div> }).collect_view()
                }}
            </dl>
        </header>

        // Your own profile only, and only while you edit it.
        <Show when=move || own.get() && editing.get()>
            <EditProfile shown=shown editing=editing />
        </Show>

        <div class="pn-layout" class:pn-own=move || own.get()>
            <div class="pn-main">
                <section class="g-section" aria-labelledby="pn-live-h">
                    <div class="g-head"><h2 id="pn-live-h">"Live now"</h2><span class="g-rule"></span></div>
                    {move || {
                        let sessions = live.with(|n| live_of(n, &live_name));
                        if sessions.is_empty() {
                            let (headline, hint) = not_live(own.get(), &live_name);
                            view! {
                                <div class="g-glass g-empty-live pn-quiet">
                                    <span class="g-glyph" aria-hidden="true"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg></span>
                                    <div><b>{headline}</b><p>{hint}</p></div>
                                </div>
                            }.into_any()
                        } else {
                            view! { <div class="g-rail">{sessions.into_iter().map(|s| view! { <LiveCard session=s /> }).collect_view()}</div> }.into_any()
                        }
                    }}
                </section>

                <section class="g-section" aria-labelledby="pn-made-h">
                    <div class="g-head">
                        <h2 id="pn-made-h">"In the Gallery"</h2>
                        {move || made.with(|shelf| match shelf {
                            Shelf::Ready(list) if !list.is_empty() => Some(view! { <span class="g-chip pn-count">{spaces_words(list.len())}</span> }),
                            _ => None,
                        })}
                        <span class="g-rule"></span>
                    </div>
                    {move || match made.get() {
                        Shelf::Loading => view! {
                            <div class="g-grid" role="status" aria-busy="true" aria-label="Loading the Spaces">
                                {(0..3).map(|_| view! { <div class="g-card g-skel-card"><div class="g-skel g-skel-media"></div><div class="g-body"><span class="g-skel w70"></span><span class="g-skel w40"></span></div></div> }).collect_view()}
                            </div>
                        }.into_any(),
                        Shelf::Failed => view! {
                            <div class="g-glass g-none" role="status">
                                <b>"The Spaces could not load right now"</b>
                                <p>"The rest of the profile is up to date. Reload the page in a minute to see them."</p>
                            </div>
                        }.into_any(),
                        Shelf::Ready(list) if list.is_empty() => {
                            let mine = own.get();
                            let (headline, hint) = nothing_made(mine, &made_name);
                            view! {
                                <div class="g-glass g-none pn-none">
                                    <b>{headline}</b>
                                    <p>{hint}</p>
                                    <div class="g-actions">
                                        {if mine {
                                            view! {
                                                <a class="g-btn g-btn-primary" href="/download">"Download Studio"</a>
                                                <a class="g-btn" href="/docs/publishing">"How publishing works"</a>
                                            }.into_any()
                                        } else {
                                            view! { <a class="g-btn" href="/gallery">"Browse the Gallery"</a> }.into_any()
                                        }}
                                    </div>
                                </div>
                            }.into_any()
                        }
                        Shelf::Ready(list) => view! {
                            <div class="g-grid pn-grid">
                                {list.into_iter().map(|l| view! { <ListingCard listing=l live=live /> }).collect_view()}
                            </div>
                        }.into_any(),
                    }}
                </section>
            </div>

            // Yours alone. Kept mounted across the session's minute-by-minute refresh, and never mounted on anyone else's profile.
            <Show when=move || own.get()>
                <AccountPanel account=account customizing=customizing />
            </Show>
        </div>

        // Your avatar, opened from the account panel: yours alone, like the panel.
        <Show when=move || own.get() && customizing.get()>
            <section class="g-glass pn-avatar-editor" aria-label="Your avatar">
                <AvatarCustomizer />
            </section>
        </Show>
    }
}

// -----------------------------------------------------------------------------
// 4. Follow, and Edit profile
// -----------------------------------------------------------------------------

/// Follow and Unfollow on someone else's profile, a link to sign in for a signed-out visitor, and nothing on your own.
/// The Followers figure moves only once the Worker said yes.
#[component]
fn FollowControl(username: String, shown: RwSignal<ProfileReply>, viewer: Signal<Viewer>, own: Memo<bool>) -> impl IntoView {
    let api_url = StoredValue::new(expect_context::<AppState>().api_url.clone());
    let username = StoredValue::new(username);
    let busy = RwSignal::new(false);
    let problem = RwSignal::new(Option::<String>::None);
    let toggle = move |_| {
        if busy.get_untracked() {
            return;
        }
        let was = shown.with_untracked(|p| p.is_following);
        busy.set(true);
        problem.set(None);
        spawn_local(async move {
            let client = ApiClient::new(api_url.get_value());
            let name = username.get_value();
            let answer = client.post::<serde_json::Value, _>(follow_path(!was), &FollowRequest { username: &name }).await;
            busy.try_set(false);
            match answer {
                Ok(_) => {
                    shown.try_update(|p| {
                        p.follower_count = followers_after(p.follower_count, was, !was);
                        p.is_following = !was;
                    });
                }
                Err(e) => {
                    problem.try_set(Some(api::failure_sentence(&e)));
                }
            }
        });
    };
    view! {
        {move || match follow_shown(&viewer.get(), own.get(), shown.with(|p| p.is_following)) {
            FollowShown::Hidden => ().into_any(),
            FollowShown::SignIn => view! { <a class="g-btn g-btn-primary pn-follow" href="/login">"Sign in to follow"</a> }.into_any(),
            FollowShown::Follow => view! {
                <button type="button" class="g-btn g-btn-primary pn-follow" aria-pressed="false" disabled=move || busy.get() on:click=toggle>"Follow"</button>
            }.into_any(),
            FollowShown::Following => view! {
                <span class="g-chip live pn-following">"Following"</span>
                <button type="button" class="g-btn pn-follow" aria-pressed="true" disabled=move || busy.get() on:click=toggle>"Unfollow"</button>
            }.into_any(),
        }}
        {move || problem.get().map(|m| view! { <p class="pn-problem" role="alert">{m}</p> })}
    }
}

/// The form behind Edit profile: display name, bio and picture address, checked here by the Worker's limits and saved by
/// the Worker, which cleans and checks them again. What it saved is what the header then shows.
#[component]
fn EditProfile(shown: RwSignal<ProfileReply>, editing: RwSignal<bool>) -> impl IntoView {
    let api_url = StoredValue::new(expect_context::<AppState>().api_url.clone());
    let start = shown.get_untracked();
    let placeholder = start.username.clone();
    let name = RwSignal::new(start.pending.display_name().or_else(|| start.display_name.clone().filter(|n| n.trim() != start.username)).unwrap_or_default());
    let bio = RwSignal::new(start.pending.bio().or_else(|| start.bio.clone()).unwrap_or_default());
    let avatar = RwSignal::new(start.avatar_url.clone().unwrap_or_default());
    let busy = RwSignal::new(false);
    let problem = RwSignal::new(Option::<String>::None);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        if busy.get_untracked() {
            return;
        }
        let (n, b, a) = (name.get_untracked(), bio.get_untracked(), avatar.get_untracked());
        if let Some((_, sentence)) = edit_problem(&n, &b, &a) {
            problem.set(Some(sentence));
            return;
        }
        problem.set(None);
        busy.set(true);
        spawn_local(async move {
            let client = ApiClient::new(api_url.get_value());
            let body = ProfileEdit { display_name: folded(&n), bio: folded(&b), avatar_url: a.trim().to_string() };
            let answer = client.post::<EditAnswer, _>("/api/account/profile", &body).await;
            busy.try_set(false);
            match answer {
                Ok(saved) => {
                    shown.try_update(|p| {
                        let next = with_saved(p, &saved);
                        *p = next;
                    });
                    editing.try_set(false);
                }
                Err(e) => {
                    if let Some(now) = answer_in_refusal(&e) {
                        shown.try_update(|p| {
                            let next = with_saved(p, &now);
                            *p = next;
                        });
                    }
                    problem.try_set(Some(save_failure(&e)));
                }
            }
        });
    };

    view! {
        <form class="g-glass pn-edit" novalidate on:submit=on_submit aria-labelledby="pn-edit-h">
            <div class="pn-account-head">
                <h2 id="pn-edit-h">"Edit profile"</h2>
                <span class="pn-private">"Everyone can see what you save here"</span>
            </div>
            <label class="pn-field">
                <span class="pn-field-label">"Display name"</span>
                <input type="text" autocomplete="nickname" placeholder=placeholder
                    prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) />
                <small>{move || format!("{} of {DISPLAY_NAME_MAX} characters. Empty shows your username.", folded(&name.get()).chars().count())}</small>
            </label>
            <label class="pn-field">
                <span class="pn-field-label">"Bio"</span>
                <textarea rows="3" prop:value=move || bio.get() on:input=move |ev| bio.set(event_target_value(&ev))></textarea>
                <small>{move || format!("{} of {BIO_MAX} characters. Line breaks become spaces.", folded(&bio.get()).chars().count())}</small>
            </label>
            <label class="pn-field">
                <span class="pn-field-label">"Picture address"</span>
                <input type="url" inputmode="url" placeholder="/assets/avatars/you.png"
                    prop:value=move || avatar.get() on:input=move |ev| avatar.set(event_target_value(&ev)) />
                <small>"A path under /assets. Empty shows your initial."</small>
            </label>
            {move || problem.get().map(|m| view! { <p class="pn-problem" role="alert">{m}</p> })}
            <div class="g-actions">
                <button type="submit" class="g-btn g-btn-primary" disabled=move || busy.get()>{move || if busy.get() { "Saving" } else { "Save" }}</button>
                <button type="button" class="g-btn" on:click=move |_| editing.set(false)>"Cancel"</button>
            </div>
        </form>
    }
}

// -----------------------------------------------------------------------------
// 5. Your account (your own profile only)
// -----------------------------------------------------------------------------

#[component]
fn AccountPanel(account: Signal<Option<Account>>, customizing: RwSignal<bool>) -> impl IntoView {
    let figure = move |pick: fn(&Account) -> String| move || account.with(|a| a.as_ref().map(pick).unwrap_or_default());
    view! {
        <aside class="g-glass pn-account" aria-labelledby="pn-account-h">
            <div class="pn-account-head">
                <h2 id="pn-account-h">"Your account"</h2>
                <span class="pn-private">
                    <svg viewBox="0 0 24 24" aria-hidden="true"><path d=LOCK_ICON /></svg>
                    "Only you can see this"
                </span>
            </div>
            <div class="pn-balances">
                <a class="pn-balance" href="/tickets">
                    <span class="pn-balance-label">"Tickets"</span>
                    <span class="pn-balance-value">{figure(|a| format_count(a.tickets))}</span>
                </a>
                <a class="pn-balance pn-bls" href="/bliss">
                    <span class="pn-balance-label">"Bliss"</span>
                    <span class="pn-balance-value">{figure(|a| format_bliss_exact(a.bliss))}</span>
                </a>
                <a class="pn-balance" href="/delta">
                    <span class="pn-balance-label">"Delta"</span>
                    <span class="pn-balance-value">{figure(|a| delta_words(a.delta))}</span>
                </a>
            </div>
            <nav class="pn-shortcuts" aria-label="Your account">
                <a href="/purchases"><b>"Purchases"</b><small>"Everything you have bought"</small></a>
                <a href="/account/cases"><b>"Cases"</b><small>"Your cases and removal requests"</small></a>
                <a href="/settings"><b>"Settings"</b><small>"Privacy, notifications and sign in"</small></a>
                <button type="button" class="pn-shortcut-btn" aria-expanded=move || customizing.get().to_string() on:click=move |_| customizing.update(|c| *c = !*c)>
                    <b>"Avatar"</b><small>{move || if customizing.get() { "Close the avatar customizer" } else { "Your character in Eustress" }}</small>
                </button>
            </nav>
            <SpendSummaryCard />
        </aside>
    }
}

/// Tickets and Bliss spent, for the account holder alone.
///
/// Reads `/api/purchases/summary`, which answers for the bearer's own account
/// and takes no account id, so the numbers cannot belong to anyone else.
#[component]
fn SpendSummaryCard() -> impl IntoView {
    let api_url = expect_context::<AppState>().api_url.clone();
    let summary = RwSignal::new(Option::<PurchaseSummary>::None);
    let failed = RwSignal::new(false);

    // An effect, so the request goes out in the browser once the card mounts.
    Effect::new(move |_| {
        let api_url = api_url.clone();
        spawn_local(async move {
            match get_purchase_summary(&ApiClient::new(api_url)).await {
                Ok(s) => summary.set(Some(s)),
                Err(_) => failed.set(true),
            }
        });
    });

    let figure = move |pick: fn(&PurchaseSummary) -> String| {
        move || summary.with(|s| s.as_ref().map(pick).unwrap_or_else(|| "\u{2026}".to_string()))
    };

    view! {
        <section class="profile-spend" aria-labelledby="profile-spend-h">
            <div class="profile-spend-head">
                <h2 id="profile-spend-h">"Your spending"</h2>
                <span class="profile-spend-private">
                    <img src="/assets/icons/shield.svg" alt="" />
                    "Only you can see this"
                </span>
            </div>
            {move || if failed.get() {
                view! {
                    <p class="profile-spend-error">"Your spending could not be loaded right now."</p>
                }.into_any()
            } else {
                view! {
                    <div class="profile-spend-stats">
                        <div class="profile-spend-stat">
                            <span class="profile-spend-label">"Tickets spent"</span>
                            <span class="profile-spend-value">
                                {figure(|s| format_count(s.totals.tickets))}
                                <span class="profile-spend-unit">"TKT"</span>
                            </span>
                        </div>
                        <div class="profile-spend-stat profile-spend-stat-bls">
                            <span class="profile-spend-label">"Bliss spent"</span>
                            <span class="profile-spend-value">
                                {figure(|s| format_bliss_exact(s.totals.bliss))}
                                <span class="profile-spend-unit">"BLS"</span>
                            </span>
                        </div>
                        <div class="profile-spend-stat">
                            <span class="profile-spend-label">"Purchases"</span>
                            <span class="profile-spend-value">{figure(|s| format_count(s.totals.purchases))}</span>
                        </div>
                    </div>
                }.into_any()
            }}
            <a href="/purchases" class="profile-spend-link">
                "See every purchase"
                <span aria-hidden="true">"\u{2192}"</span>
            </a>
        </section>
    }
}

// -----------------------------------------------------------------------------
// 6. Loading, signed out, missing, failed
// -----------------------------------------------------------------------------

/// The shapes of the profile while it loads, not made-up content.
#[component]
fn ProfileSkeleton() -> impl IntoView {
    view! {
        <div class="pn-skeleton" role="status" aria-busy="true" aria-label="Loading the profile">
            <div class="g-glass pn-head">
                <div class="pn-avatar g-skel"></div>
                <div class="pn-id"><span class="g-skel w40 tall"></span><span class="g-skel w30"></span><span class="g-skel w60"></span></div>
            </div>
            <div class="g-grid pn-grid">
                {(0..3).map(|_| view! { <div class="g-card g-skel-card"><div class="g-skel g-skel-media"></div><div class="g-body"><span class="g-skel w70"></span><span class="g-skel w40"></span></div></div> }).collect_view()}
            </div>
        </div>
    }
}

#[component]
fn ProfileSignedOut() -> impl IntoView {
    view! {
        <div class="g-glass g-none pn-state">
            <b>"Sign in to see your profile"</b>
            <p>"Your profile shows what you made and, to you alone, your account."</p>
            <div class="g-actions">
                <a class="g-btn g-btn-primary" href="/login">"Sign in"</a>
                <a class="g-btn" href="/gallery">"Browse the Gallery"</a>
            </div>
        </div>
    }
}

#[component]
fn ProfileMissing(name: String) -> impl IntoView {
    view! {
        <div class="g-glass g-none pn-state">
            <b>{format!("There is no profile for @{name}")}</b>
            <p>"Check the spelling, or find the person through the Gallery's search."</p>
            <div class="g-actions"><a class="g-btn" href="/gallery">"Browse the Gallery"</a></div>
        </div>
    }
}

#[component]
fn ProfileFailed(why: String, retry: Callback<()>) -> impl IntoView {
    view! {
        <div class="g-glass g-none pn-state" role="alert">
            <b>"This profile could not load"</b>
            <p>{why}</p>
            <div class="g-actions"><button type="button" class="g-btn g-btn-primary" on:click=move |_| retry.run(())>"Try again"</button></div>
        </div>
    }
}

// -----------------------------------------------------------------------------
// 7. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::User;

    const SOURCE: &str = include_str!("profile.rs");
    const CSS: &str = include_str!("../../style/profile.css");

    /// The Worker's answer for a profile, in its own shape (handleUserProfile), with the fields this page leaves alone.
    fn worker_reply(verified: bool, badges: &str) -> ProfileReply {
        serde_json::from_str(&format!(
            r#"{{"username":"ada","display_name":"ada","bio":"I model bridges.","avatar_url":null,"banner_url":null,
                "join_date":"October 2026","follower_count":12,"following_count":3,"friend_count":2,"simulation_count":4,
                "total_plays":1534,"favorite_count":9,"inventory_count":11,"badges":{badges},"is_verified":{verified},
                "is_following":false,"discord_linked":false,"created_at":"2026-10-01T00:00:00.000Z"}}"#
        ))
        .expect("the Worker's profile answer parses")
    }

    fn user(name: &str) -> User {
        serde_json::from_str(&format!(
            r#"{{"id":"00000000-0000-0000-0000-000000000001","username":"{name}","email":"private@example.com","avatar_url":null,
                "discord_id":null,"bliss_balance":12.5,"ticket_balance":4321,"created_at":"2026-01-01T00:00:00Z"}}"#
        ))
        .expect("a session user parses")
    }

    fn space(id: &str, author: Option<&str>) -> Listing {
        Listing { id: id.to_string(), name: format!("Space {id}"), author_name: author.map(str::to_string), ..Default::default() }
    }

    fn mine_account() -> Account {
        Account { tickets: 4321, bliss: 12.5, delta: DeltaShown::Whole(77) }
    }

    /// The profile body, rendered on the server the way the prerender renders a page.
    fn render(profile: ProfileReply, account: Option<Account>, shelf: Shelf, live: LiveNow) -> String {
        render_as(Viewer::SignedIn, profile, account, shelf, live)
    }

    /// The same, for a viewer who is signed in, signed out, or not known yet.
    fn render_as(viewer: Viewer, profile: ProfileReply, account: Option<Account>, shelf: Shelf, live: LiveNow) -> String {
        use leptos::tachys::view::RenderHtml;
        let owner = Owner::new();
        owner.with(|| {
            provide_context(AppState::new());
            crate::components::play_chooser::provide_play_chooser();
            let made = RwSignal::new(shelf);
            let live = RwSignal::new(live);
            view! { <ProfileBody profile=profile account=Signal::stored(account) viewer=Signal::stored(viewer) made=made live=live /> }.to_html()
        })
    }

    #[cfg(feature = "ssr")]
    fn render_app_at(url: &str) -> String {
        use hydration_context::{SharedContext, SsrSharedContext};
        use leptos::tachys::view::RenderHtml;
        use leptos_router::location::RequestUrl;
        // The current page starts its fetch while it is built; the futures executor parks it.
        let _ = leptos::task::Executor::init_futures_executor();
        let shared: std::sync::Arc<dyn SharedContext + Send + Sync> = std::sync::Arc::new(SsrSharedContext::new());
        let owner = Owner::new_root(Some(shared));
        let html = owner.with(|| {
            provide_context(RequestUrl::new(url));
            crate::app::App().to_html()
        });
        owner.cleanup();
        html
    }

    /// The CSS rule for one selector, from its brace to the next.
    fn rule<'a>(css: &'a str, selector: &str) -> &'a str {
        let at = css.find(&format!("{selector} {{")).unwrap_or_else(|| panic!("{selector} has a rule in profile.css"));
        let rest = &css[at..];
        &rest[..rest.find('}').expect("the rule closes")]
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn every_profile_address_shows_this_page_and_nothing_of_the_old_one() {
        for url in ["/profile", "/profile/ada", "/profile?design=next", "/profile/ada?design=current", "/profile?tab=x"] {
            let html = render_app_at(url);
            assert!(html.contains("pn-page"), "{url} shows the profile page");
            for gone in [concat!("page-profile-", "industrial"), concat!("profile-", "tab"), concat!("profile-header-", "industrial")] {
                assert!(!html.contains(gone), "{url} shows nothing of the old page ({gone})");
            }
        }
    }

    #[test]
    fn your_account_is_built_only_for_your_own_profile() {
        assert!(is_own(Some("ada"), "ada"));
        assert!(!is_own(Some("bob"), "ada"));
        assert!(!is_own(Some("Ada"), "ada"), "the Worker's username, exactly");
        assert!(!is_own(None, "ada"));
        assert!(!is_own(Some(""), ""), "a profile that has not loaded is nobody's");
        let signed_in_as_bob = AuthState::Authenticated(user("bob"));
        assert_eq!(account_of(&signed_in_as_bob, &DeltaState::Unknown, "ada"), None);
        assert_eq!(account_of(&AuthState::Unauthenticated, &DeltaState::Unknown, "ada"), None);
        let signed_in_as_ada = AuthState::Authenticated(user("ada"));
        let mine = account_of(&signed_in_as_ada, &DeltaState::Absent, "ada").expect("ada's own profile");
        assert_eq!((mine.tickets, mine.bliss, mine.delta), (4321, 12.5, DeltaShown::Unavailable));
    }

    #[test]
    fn nothing_private_renders_on_another_persons_profile() {
        let profile = worker_reply(true, "[]");
        let theirs = render(profile.clone(), None, Shelf::Ready(vec![]), LiveNow::default());
        for private in ["Your account", "Only you can see this", "This is you", "/purchases", "/account/cases", "/settings", "/tickets", "/bliss", "/delta", "profile-spend", "pn-account", "4,321", "12.50", "private@example.com", "Download Studio", "Edit profile", "pn-edit", "Your character in Eustress", "pn-shortcut-btn"] {
            assert!(!theirs.contains(private), "someone else's profile shows {private:?}");
        }
        // The fields the Worker sends that describe private activity are never drawn.
        for count in ["Favorites", "Inventory", "Friends"] {
            assert!(!theirs.contains(count), "someone else's profile shows {count}");
        }
        let mine = render(profile, Some(mine_account()), Shelf::Ready(vec![]), LiveNow::default());
        for yours in ["Your account", "Only you can see this", "/purchases", "/account/cases", "/settings", "/tickets", "/bliss", "/delta", "4,321", "12.50", "77", "profile-spend", "Edit profile", "/account/cases", "Your character in Eustress"] {
            assert!(mine.contains(yours), "your own profile shows {yours:?}");
        }
    }

    #[test]
    fn the_verified_mark_comes_only_from_the_workers_is_verified_field() {
        let verified_badge = r#"[{"id":"verified","name":"Verified","icon":"x","description":"Identity verified via KYC"},{"id":"early","name":"Early Adopter","icon":"x","description":"Joined before 2030"}]"#;
        let not_verified = worker_reply(false, verified_badge);
        assert!(!not_verified.is_verified);
        let html = render(not_verified.clone(), None, Shelf::Loading, LiveNow::default());
        assert!(!html.contains("Verified"), "a badge alone never makes the mark");
        assert!(!html.contains("pn-verified"));
        assert!(html.contains("Early Adopter"), "the other badges are chips");
        assert_eq!(shown_badges(&not_verified).iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), ["early"]);

        let verified = worker_reply(true, "[]");
        let html = render(verified, None, Shelf::Loading, LiveNow::default());
        assert!(html.contains("pn-verified") && html.contains("Verified"), "is_verified draws the mark with no badge at all");

        let silent: ProfileReply = serde_json::from_str(r#"{"username":"ada"}"#).unwrap();
        assert!(!silent.is_verified, "a Worker that does not say is not a yes");
    }

    #[test]
    fn a_profile_with_nothing_in_the_gallery_says_so_and_a_loading_or_failed_list_never_claims_it() {
        let theirs = render(worker_reply(false, "[]"), None, Shelf::Ready(vec![]), LiveNow::default());
        assert!(theirs.contains("@ada has nothing in the Gallery yet"));
        assert!(theirs.contains("Browse the Gallery"));
        assert!(theirs.contains("Not live right now"), "the live section has its own empty state");
        assert!(!theirs.contains("g-card"), "no card is drawn for nothing");

        let mine = render(worker_reply(false, "[]"), Some(mine_account()), Shelf::Ready(vec![]), LiveNow::default());
        assert!(mine.contains("You have nothing in the Gallery yet"));
        assert!(mine.contains("You are not live right now"));
        assert!(mine.contains("href=\"/download\""));

        let loading = render(worker_reply(false, "[]"), None, Shelf::Loading, LiveNow::default());
        assert!(!loading.contains("nothing in the Gallery"), "a list still loading is not an empty one");
        let failed = render(worker_reply(false, "[]"), None, Shelf::Failed, LiveNow::default());
        assert!(!failed.contains("nothing in the Gallery"));
        assert!(failed.contains("The Spaces could not load right now"));
        assert!(!failed.contains("<dt>Spaces</dt>"), "an unknown count is left out, not shown as zero");
    }

    #[test]
    fn their_spaces_are_the_gallerys_own_cards_and_their_live_sessions_the_gallerys_live_cards() {
        let made = made_by(&[space("a", Some("Ada")), space("b", Some("bob")), space("c", None), space("d", Some("ada "))], "ada");
        assert_eq!(made.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["a", "d"], "theirs alone, in the Worker's order");
        let live = LiveNow {
            sessions: vec![
                Session { id: "a".to_string(), name: "Space a".to_string(), author_name: Some("ada".to_string()), players: 2, max_players: 8, ..Default::default() },
                Session { id: "b".to_string(), name: "Space b".to_string(), author_name: Some("bob".to_string()), players: 1, max_players: 4, ..Default::default() },
            ],
            players_total: 3,
            sessions_total: 2,
        };
        assert_eq!(live_of(&live, "ada").len(), 1);
        let html = render(worker_reply(false, "[]"), None, Shelf::Ready(made), live);
        assert_eq!(html.matches("class=\"g-card").count(), 2, "one Gallery card per Space");
        assert!(html.contains("g-play"), "with the Gallery card's Play button");
        assert!(html.contains("2 Spaces"));
        assert!(html.contains("g-live-card") && html.contains("2/8"), "the live session, with its seats");
        assert!(!html.contains("Space b"), "someone else's session is not theirs");
        assert!(!html.contains("Not live right now"));
    }

    #[test]
    fn the_figures_are_the_workers_numbers() {
        let p = worker_reply(false, "[]");
        let ready = figures_of(&p, &Shelf::Ready(vec![space("a", Some("ada"))]));
        assert_eq!(ready, vec![("Spaces", "1".to_string()), ("Visits", "1,534".to_string()), ("Followers", "12".to_string()), ("Following", "3".to_string())]);
        assert_eq!(figures_of(&p, &Shelf::Loading)[0], ("Spaces", "\u{2026}".to_string()));
        assert_eq!(figures_of(&p, &Shelf::Failed).len(), 3);
        assert_eq!(joined_words("October 2026").as_deref(), Some("Joined October 2026"));
        assert_eq!(joined_words("  "), None);
        assert_eq!(spaces_words(1), "1 Space");
        assert_eq!(spaces_words(1200), "1,200 Spaces");
        assert_eq!(delta_words(DeltaShown::Whole(1500)), "1,500");
    }

    #[test]
    fn the_avatar_is_a_safe_picture_or_a_letter_and_never_squishes() {
        assert_eq!(safe_avatar(Some("/assets/a.png")).as_deref(), Some("/assets/a.png"));
        assert_eq!(safe_avatar(Some("/assets/avatars/a_b-c.png")).as_deref(), Some("/assets/avatars/a_b-c.png"));
        for bad in [
            "https://cdn.eustress.dev/a.png", "https://api.eustress.dev/api/simulations/abc/thumbnail", "https://eustress.dev/a.png",
            "javascript:alert(1)", "http://x/a.png", "//evil.example/a.png", "data:image/png;base64,AA", "",
            "https://evil.example/a.png", "https://eustress.dev.evil.example/a.png", "https://evileustress.dev/a.png",
            "https://user:pw@eustress.dev/a.png", "https://eustress.dev:8443/a.png", "/assets/../secret", "/assets//x",
            "/api/simulations/x/thumbnail", "/assets/a b.png", "/assets/<x>.png", "/sample/a.png",
        ] {
            assert_eq!(safe_avatar(Some(bad)), None, "{bad}");
        }
        assert_eq!(monogram("ada"), "A");
        assert_eq!(monogram("_x"), "X");
        assert_eq!(monogram(""), "?");
        let frame = rule(CSS, ".pn-avatar");
        assert!(frame.contains("flex: none") && frame.contains("aspect-ratio: 1 / 1"), "the frame keeps its square: {frame}");
        let img = rule(CSS, ".pn-avatar img");
        assert!(img.contains("object-fit: cover") && img.contains("width: 100%") && img.contains("height: 100%"), "the picture fills it without stretching: {img}");
        let html = render(worker_reply(false, "[]"), None, Shelf::Loading, LiveNow::default());
        assert!(html.contains("pn-monogram") && html.contains(">A<"), "no picture, the letter");
    }

    /// The avatar area for this profile picture, with this address already failed, rendered on the server.
    fn avatar_html(avatar: Option<&str>, failed: Option<&str>) -> String {
        use leptos::tachys::view::RenderHtml;
        let owner = Owner::new();
        owner.with(|| {
            let mut profile = worker_reply(false, "[]");
            profile.avatar_url = avatar.map(str::to_string);
            avatar_face(&profile, RwSignal::new(failed.map(str::to_string))).to_html()
        })
    }

    #[test]
    fn an_avatar_that_will_not_load_is_the_letter_a_profile_without_one_draws() {
        let shown = avatar_html(Some("/assets/avatars/a.png"), None);
        assert!(shown.contains("<img") && shown.contains("src=\"/assets/avatars/a.png\"") && !shown.contains("pn-monogram"), "{shown}");
        let none = avatar_html(None, None);
        assert!(none.contains("pn-monogram") && none.contains(">A<") && !none.contains("<img"), "{none}");
        // The address the img's on:error recorded: the profile then draws exactly what it draws with no avatar.
        let failed = avatar_html(Some("/assets/avatars/a.png"), Some("/assets/avatars/a.png"));
        assert_eq!(failed, none, "an avatar that failed shows the letter, not the browser's broken-image icon");
        // A different picture (an edit was saved) is a new try.
        let replaced = avatar_html(Some("/assets/avatars/b.png"), Some("/assets/avatars/a.png"));
        assert!(replaced.contains("src=\"/assets/avatars/b.png\"") && !replaced.contains("pn-monogram"), "{replaced}");
    }

    #[test]
    fn the_avatar_has_an_error_handler_that_records_its_own_address() {
        let page = SOURCE.split(concat!("#[cfg", "(test)]")).next().unwrap();
        let at = page.find("width=\"128\"").expect("the avatar's picture");
        let start = page[..at].rfind("<img").expect("an img tag");
        let tag = &page[start..start + page[start..].find("/>").expect("the tag ends")];
        assert!(tag.contains("on:error=move |_| failed.set(Some(url.clone()))"), "{tag}");
        assert!(page.contains("filter(|src| failed.with(|bad| bad.as_deref() != Some(src.as_str())))"), "a failed address is not drawn again");
        // One signal for each profile body, kept apart from the profile so a saved edit does not reset it.
        assert!(page.contains("let avatar_failed = RwSignal::new(None::<String>);"));
        assert!(page.contains("{move || shown.with(|p| avatar_face(p, avatar_failed))}"));
        assert_eq!(page.matches("width=\"128\"").count(), 1, "one avatar picture");
    }

    #[test]
    fn the_copy_has_no_em_or_en_dashes() {
        for dash in ['\u{2014}', '\u{2013}'] {
            assert!(!SOURCE.contains(dash), "profile.rs");
            assert!(!CSS.contains(dash), "profile.css");
            for html in [
                render(worker_reply(true, "[]"), Some(mine_account()), Shelf::Ready(vec![]), LiveNow::default()),
                render(worker_reply(false, "[]"), None, Shelf::Ready(vec![]), LiveNow::default()),
                render(worker_reply(false, "[]"), None, Shelf::Failed, LiveNow::default()),
            ] {
                assert!(!html.contains(dash), "the rendered profile");
            }
        }
    }

    #[test]
    fn a_missing_profile_and_the_address_are_read_plainly() {
        assert_eq!(target_of(Some("ada"), &AuthState::Unknown), Target::Named("ada".to_string()));
        assert_eq!(target_of(Some("  "), &AuthState::Unauthenticated), Target::SignedOut);
        assert_eq!(target_of(None, &AuthState::Unknown), Target::Waiting);
        assert_eq!(target_of(None, &AuthState::Authenticated(user("ada"))), Target::Named("ada".to_string()));
        assert_eq!(load_of(Err(ApiError::NotFound), "ghost"), Load::Missing("ghost".to_string()));
        assert_eq!(load_of(Err(ApiError::Server { status: 400, message: String::new() }), "x"), Load::Missing("x".to_string()));
        assert!(matches!(load_of(Err(ApiError::Network("down".to_string())), "ada"), Load::Failed(_)));
        assert_eq!(load_of(Err(ApiError::NotFound), &"z".repeat(500)), Load::Missing("z".repeat(40)));
    }

    #[test]
    fn follow_is_for_someone_elses_profile_and_a_signed_out_visitor_is_sent_to_sign_in() {
        assert_eq!(follow_shown(&Viewer::SignedIn, true, false), FollowShown::Hidden, "never on your own profile");
        assert_eq!(follow_shown(&Viewer::SignedIn, true, true), FollowShown::Hidden);
        assert_eq!(follow_shown(&Viewer::Unknown, false, false), FollowShown::Hidden, "nothing until the session is known");
        assert_eq!(follow_shown(&Viewer::SignedOut, false, false), FollowShown::SignIn);
        assert_eq!(follow_shown(&Viewer::SignedIn, false, false), FollowShown::Follow);
        assert_eq!(follow_shown(&Viewer::SignedIn, false, true), FollowShown::Following);

        let theirs = || worker_reply(false, "[]");
        let signed_out = render_as(Viewer::SignedOut, theirs(), None, Shelf::Loading, LiveNow::default());
        assert!(signed_out.contains("href=\"/login\"") && signed_out.contains("Sign in to follow"), "a link to sign in");
        assert!(!signed_out.contains("<button type=\"button\" class=\"g-btn g-btn-primary pn-follow\""), "no Follow button signed out");

        let follow = render_as(Viewer::SignedIn, theirs(), None, Shelf::Loading, LiveNow::default());
        assert!(follow.contains(">Follow</button>") && follow.contains("aria-pressed=\"false\""));
        let mut followed = theirs();
        followed.is_following = true;
        let unfollow = render_as(Viewer::SignedIn, followed, None, Shelf::Loading, LiveNow::default());
        assert!(unfollow.contains(">Unfollow</button>") && unfollow.contains("Following") && unfollow.contains("aria-pressed=\"true\""));

        let unknown = render_as(Viewer::Unknown, theirs(), None, Shelf::Loading, LiveNow::default());
        assert!(!unknown.contains("pn-follow"));
        let mine = render_as(Viewer::SignedIn, theirs(), Some(mine_account()), Shelf::Loading, LiveNow::default());
        assert!(!mine.contains("pn-follow") && !mine.contains("Sign in to follow"), "no Follow on your own profile");
        assert!(mine.contains(">Edit profile</button>"));
    }

    #[test]
    fn follow_and_unfollow_call_the_workers_routes_and_the_followers_figure_moves_once() {
        assert_eq!(follow_path(true), "/api/social/follow");
        assert_eq!(follow_path(false), "/api/social/unfollow");
        assert_eq!(serde_json::to_string(&FollowRequest { username: "ada" }).unwrap(), r#"{"username":"ada"}"#);
        assert_eq!(followers_after(12, false, true), 13);
        assert_eq!(followers_after(12, true, false), 11);
        assert_eq!(followers_after(0, true, false), 0, "never below zero");
        assert_eq!(followers_after(12, true, true), 12, "the same state is the same count");
        assert_eq!(followers_after(12, false, false), 12);
        let mut reply = worker_reply(false, "[]");
        assert!(!reply.is_following, "the Worker's is_following is read");
        reply = serde_json::from_str(r#"{"username":"ada","follower_count":4,"is_following":true}"#).unwrap();
        assert!(reply.is_following);
    }

    #[test]
    fn an_edit_is_checked_by_the_workers_limits_and_what_the_worker_saved_is_shown() {
        assert_eq!(edit_problem(&"x".repeat(32), &"y".repeat(280), "/assets/avatars/a.png"), None);
        assert_eq!(edit_problem(&"x".repeat(33), "", "").map(|p| p.0), Some("display_name"));
        assert_eq!(edit_problem("", &"y".repeat(281), "").map(|p| p.0), Some("bio"));
        assert_eq!(edit_problem("  Ada   Builds  ", &format!("a{}b", " ".repeat(400)), ""), None, "whitespace is folded before it is counted");
        assert_eq!(edit_problem(&"\u{e9}".repeat(32), "", ""), None, "characters, not bytes");
        assert_eq!(edit_problem("", "", "https://evil.example/a.png").map(|p| p.0), Some("avatar_url"));
        assert_eq!(edit_problem("", "", &format!("/assets/{}", "a".repeat(600))).map(|p| p.0), Some("avatar_url"));
        assert_eq!(edit_problem("", "", "https://api.eustress.dev/api/simulations/abc/cover").map(|p| p.0), Some("avatar_url"), "no address, ours included");
        assert_eq!(edit_problem("", "", "   "), None, "an empty picture clears it");

        let body = ProfileEdit { display_name: folded(" Ada  B "), bio: folded("one\ntwo"), avatar_url: String::new() };
        assert_eq!(serde_json::to_string(&body).unwrap(), r#"{"display_name":"Ada B","bio":"one two","avatar_url":""}"#);

        let answer: EditAnswer = serde_json::from_str(r#"{"ok":true,"profile":{"display_name":"","bio":"Bridges.","avatar_url":null},"changed":true}"#).unwrap();
        let mut before = worker_reply(false, "[]");
        before.display_name = Some("Old".to_string());
        before.avatar_url = Some("/assets/a.png".to_string());
        let after = with_saved(&before, &answer);
        assert_eq!(name_of(&after), "ada", "a cleared display name shows the username");
        assert_eq!(after.bio.as_deref(), Some("Bridges."));
        assert_eq!(after.avatar_url, None);
        assert_eq!(save_failure(&ApiError::NotFound), "Editing a profile is not available on this server yet.");
        for (status, body, said) in [
            (400, r#"{"error":"That display name is kept for Eustress staff and services. Choose another.","code":"reserved_name","field":"display_name"}"#, "That display name is kept for Eustress staff and services. Choose another."),
            (400, r#"{"error":"That display name reads as another account's username. Choose another.","code":"name_is_a_username"}"#, "That display name reads as another account's username. Choose another."),
            (429, r#"{"error":"You can change your profile 10 times an hour. Try again later.","code":"rate_limited"}"#, "You can change your profile 10 times an hour. Try again later."),
        ] {
            assert_eq!(save_failure(&ApiError::Server { status, message: body.to_string() }), said, "the Worker's sentence is shown as it is");
        }
        let refused = ApiError::Server { status: 403, message: r#"{"error":"Verify your identity to keep using your account","code":"kyc_required"}"#.to_string() };
        assert_eq!(save_failure(&refused), api::failure_sentence(&refused), "any other refusal reads as every other failed call on the site");
        assert_ne!(save_failure(&refused), save_failure(&ApiError::NotFound));
    }

    #[test]
    fn the_old_pages_invented_tabs_and_stand_ins_are_gone_and_there_is_one_profile_page() {
        // McKale, 2026-10-09: the redesign is the profile page; the invented Friends and Inventory tabs, the stand-in stats and
        // the Follow button that changed nothing are gone. Written in two halves, so this test does not find itself.
        for invented in [
            concat!("fn Friend", "Card"), concat!("fn Inventory", "Item"), concat!("\"Gamer", "Pro\""), concat!("\"Golden ", "Sword\""),
            concat!("profile-", "tab"), concat!("btn-", "follow"), concat!("page-profile-", "industrial"), concat!("fn format_", "number"),
        ] {
            assert!(!SOURCE.contains(invented), "profile.rs still has {invented}");
        }
        assert!(!include_str!("mod.rs").contains(concat!("pub mod profile", "_next")), "no second profile page");
        let app = include_str!("../app.rs");
        assert!(!app.contains(concat!("Profile", "Route")) && !app.contains(concat!("design", "=")), "no switch between designs");
        assert_eq!(app.matches("view=ProfilePage />").count(), 2, "both profile addresses open this page");
    }

    #[test]
    fn the_username_is_beside_every_display_name() {
        let mut p = worker_reply(false, "[]");
        p.display_name = Some("Ada Builds".to_string());
        let html = render(p, None, Shelf::Loading, LiveNow::default());
        let name = html.find(">Ada Builds</h1>").expect("the display name");
        let handle = html.find("<div class=\"pn-handle\">@ada</div>").expect("the username beside it");
        assert!(handle > name && handle - name < 400, "the @username follows the name in the same block");
        assert_eq!(handle_of("ada"), "@ada");
        // The friend cards show a display name too: the username is beside it there as well.
        let friends = include_str!("friends.rs");
        assert_eq!(friends.matches("<span class=\"friend-handle\">{handle}</span>").count(), 2, "both friend lists");
        assert_eq!(friends.matches("let handle = crate::pages::profile::handle_of(&friend.username);").count(), 2);
        assert!(friends.contains("{crate::pages::profile::handle_of(&uname)}"), "the friend search shows @username");
    }

    #[test]
    fn what_waits_for_review_is_shown_to_the_owner_alone_marked_pending() {
        let mut p = worker_reply(false, "[]");
        p.pending = serde_json::from_str(r#"{"display_name":"Ada Builds","bio":"Cranes and the loads they carry."}"#).unwrap();
        let mine = render(p.clone(), Some(mine_account()), Shelf::Loading, LiveNow::default());
        assert!(mine.contains("Pending review") && mine.contains(">Ada Builds<") && mine.contains("Cranes and the loads they carry."), "the owner sees what waits");
        assert!(mine.contains(">ada</h1>"), "the approved name (here the username) stays the heading");
        // Someone else's profile never shows it, even if an answer carried it.
        let theirs = render(p, None, Shelf::Loading, LiveNow::default());
        assert!(!theirs.contains("Pending review") && !theirs.contains("Ada Builds") && !theirs.contains("Cranes and the loads"));

        let answer: EditAnswer = serde_json::from_str(r#"{"ok":true,"profile":{"display_name":"","bio":"","avatar_url":null},"pending":{"display_name":"Ada Builds"},"changed":true}"#).unwrap();
        let after = with_saved(&worker_reply(false, "[]"), &answer);
        assert_eq!(name_of(&after), "ada");
        assert_eq!(after.pending.display_name().as_deref(), Some("Ada Builds"));
        assert_eq!(after.pending.bio(), None);

        let refused = ApiError::Server { status: 400, message: r#"{"error":"Your bio was not accepted: it reads as spam.","code":"refused","field":"bio","category":"spam","profile":{"display_name":"Ada","bio":"","avatar_url":null},"pending":{}}"#.to_string() };
        let now = answer_in_refusal(&refused).expect("a refusal says what the profile now is");
        assert_eq!(now.pending, PendingFields::default(), "the refused value no longer waits");
        assert_eq!(save_failure(&refused), "Your bio was not accepted: it reads as spam.");
        assert!(answer_in_refusal(&ApiError::Server { status: 400, message: r#"{"code":"too_long"}"#.to_string() }).is_none());
    }
    #[test]
    fn the_owner_is_told_why_a_value_waits_and_nobody_else_is() {
        let mut p = worker_reply(false, "[]");
        p.pending = serde_json::from_str(r#"{"display_name":"Ada Builds","bio":"Bridges."}"#).unwrap();
        p.pending_notes = serde_json::from_str(r#"{"display_name":{"state":"review"},"bio":{"state":"unavailable","reason":"Profile text can be checked 50 times a day, and this account has reached that."}}"#).unwrap();
        let mine = render(p.clone(), Some(mine_account()), Shelf::Loading, LiveNow::default());
        assert!(mine.contains("A person will look at this before anyone else sees it."), "review: a person will look");
        assert!(!mine.contains("runs again when you next save"), "review is not a retry");
        assert!(mine.contains("Profile text can be checked 50 times a day"), "unavailable: the check's own sentence");
        let theirs = render(p, None, Shelf::Loading, LiveNow::default());
        assert!(!theirs.contains("A person will look") && !theirs.contains("50 times a day") && !theirs.contains("pn-pending"));

        assert_eq!(note_words(None), "Waiting for the check before anyone else sees it.");
        let plain = PendingNote { state: "unavailable".to_string(), reason: None };
        assert_eq!(note_words(Some(&plain)), "The check could not run. It runs again when you next save.");
        let blank = PendingNote { state: "unavailable".to_string(), reason: Some("  ".to_string()) };
        assert_eq!(note_words(Some(&blank)), "The check could not run. It runs again when you next save.");
        let answer: EditAnswer = serde_json::from_str(r#"{"ok":true,"profile":{"display_name":"","bio":"","avatar_url":null},"pending":{"bio":"Bridges."},"pending_notes":{"bio":{"state":"review"}},"changed":true}"#).unwrap();
        let after = with_saved(&worker_reply(false, "[]"), &answer);
        assert_eq!(after.pending_notes.bio.map(|n| n.state).as_deref(), Some("review"));
    }
}
