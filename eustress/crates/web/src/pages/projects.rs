// =============================================================================
// Eustress Web - Projects
// =============================================================================
// The signed-in account's own Spaces, in the Gallery's look: where each one stands in review, its tags, who can see it, what it
// sells, and the way to play, edit, configure, archive or sell it. Every name, picture and number is something the Worker said:
//
//   GET   /api/projects                            the account's own Spaces. Signed out, the Worker answers an empty list, so
//                                                  the page asks the visitor to sign in rather than saying there is nothing.
//   GET   /api/commerce/account (test mode)        what each published Space can sell: a card's chip and Sell button
//                                                  (project_sell.rs)
//   POST  /api/simulations/{id}/archive|unarchive  a card's Archive and Unarchive; the list is read again afterwards
//   PATCH /api/simulations/{id}/tags               the tag editor in a Space's settings (project_tags.rs)
//
// Read and left out on purpose: `favorite_count`, which the Worker sets to 0 at publish and never counts, so the old count of
// favorites in the header and on every card said 0. The panel about an AI training export is gone too: no route serves that export (its
// stream figures are in docs/benchmarks/BENCHMARK_EUSTRESSSTREAM.md), and the API keys it linked to stay on /ai.
//
// The API keys section at the end of this file is drawn by the AI page (/ai), not by this page, and is unchanged.
//
// Table of Contents:
// 1. Data types and the pure rules (tested)
// 2. The page
// 3. The body: header, toolbar, grid and states
// 4. A project's card
// 5. Loading, failed, signed-out and empty states
// 6. API keys (drawn by /ai)
// 7. Tests
// =============================================================================

use leptos::prelude::*;
use crate::api::{ApiClient};
use crate::api::{commerce_error_text, get_commerce_account, CommerceMode, SellerSimulation};
use crate::api::catalog::published_words;
use crate::components::action_menu::{Action, ActionMenu, Item};
use crate::components::play_chooser::now_ms;
use crate::components::{CentralNav, Footer};
use crate::state::{AppState, AuthState};
use crate::utils::{after_event, format_count};
use super::review_panel::ReviewPanel;
use super::project_sell::{seller_for, SellButton, SellChip, SellNote};
use super::project_tags::TagEditor;
use crate::tags::{gallery_href, query_matches_tag, suggested_label};

// -----------------------------------------------------------------------------
// 1. Data types and the pure rules
// -----------------------------------------------------------------------------

/// Project/Place status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpaceStatus {
    Published,
    Draft,
    Archived,
    UnderReview,
    /// Review decided against listing it.
    NotListed,
    /// Review asked for changes before it can be listed.
    ChangesRequested,
    /// Passed review, and the author keeps it private.
    Private,
}

impl SpaceStatus {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::Draft => "draft",
            Self::Archived => "archived",
            Self::UnderReview => "review",
            Self::NotListed => "not_listed",
            Self::ChangesRequested => "changes_requested",
            Self::Private => "private",
        }
    }

    pub(crate) fn display_name(&self) -> &'static str {
        match self {
            Self::Published => "Published",
            Self::Draft => "Draft",
            Self::Archived => "Archived",
            Self::UnderReview => "Under Review",
            Self::NotListed => "Not listed",
            Self::ChangesRequested => "Changes requested",
            Self::Private => "Private",
        }
    }

    /// The status the API reports for a project. Held and quarantined cases
    /// both arrive as `in_review` and are not told apart here.
    pub(crate) fn from_api(status: &str) -> Self {
        match status {
            "published" => Self::Published,
            "draft" | "rejected" => Self::Draft,
            "archived" => Self::Archived,
            "not_listed" => Self::NotListed,
            "changes_requested" => Self::ChangesRequested,
            "approved_private" => Self::Private,
            "in_review" | "appeal_in_review" | "unreviewed" | "review"
            | "pending" | "classifying" | "held" | "appealed" | "quarantined" => Self::UnderReview,
            _ => Self::Draft,
        }
    }

    /// Whether the review panel has something to say about this project.
    fn has_review(&self) -> bool {
        matches!(self, Self::UnderReview | Self::NotListed | Self::ChangesRequested | Self::Private)
    }
}

/// User's place/project.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub id: String,
    pub name: String,
    pub description: String,
    pub thumbnail_url: Option<String>,
    pub status: SpaceStatus,
    pub visits: u64,
    pub favorites: u64,
    pub max_players: u32,
    /// The tags that describe it, and who chose them (`"author"` or `"judge"`; absent means the author).
    pub tags: Vec<String>,
    pub tags_source: Option<String>,
    pub updated_at: String,
    pub created_at: String,
    pub is_public: bool,
}

/// API response types
#[derive(Clone, Debug, serde::Deserialize)]
pub struct ProjectsResponse {
    pub projects: Vec<ProjectData>,
    pub total: u64,
    pub page: u32,
    pub limit: u32,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct ProjectData {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub thumbnail_url: Option<String>,
    pub status: String,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub tags_source: Option<String>,
    pub max_players: u32,
    pub is_public: bool,
    pub version: u32,
    pub play_count: u64,
    pub favorite_count: u64,
    pub last_edited: String,
    pub created_at: String,
    pub published_at: Option<String>,
    pub storage_url: Option<String>,
}

/// One part of the page's data: still loading, here, or its route failed (with the words for why).
#[derive(Clone, Debug, PartialEq)]
pub enum Load<T> {
    Loading,
    Ready(T),
    Failed(String),
}

/// Who is looking: the sign-in is still being checked, nobody is signed in, or the account is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewer {
    Unknown,
    SignedOut,
    SignedIn,
}

/// The status chips over the grid: the four that are always offered, then the rarer ones only when a Space has that status.
const STATUS_CHIPS: [(&str, &str, bool); 8] = [
    ("all", "All", true),
    ("published", "Published", true),
    ("draft", "Drafts", true),
    ("review", "In review", true),
    ("archived", "Archived", true),
    ("private", "Private", false),
    ("changes_requested", "Changes requested", false),
    ("not_listed", "Not listed", false),
];

/// How many Spaces a status chip stands for.
pub fn status_count(list: &[Place], status: &str) -> usize {
    list.iter().filter(|p| status == "all" || p.status.as_str() == status).count()
}

/// The chips to offer, each with its count.
pub fn status_chips(list: &[Place]) -> Vec<(&'static str, &'static str, usize)> {
    STATUS_CHIPS
        .iter()
        .map(|(key, label, always)| (*key, *label, *always, status_count(list, key)))
        .filter(|(_, _, always, n)| *always || *n > 0)
        .map(|(key, label, _, n)| (key, label, n))
        .collect()
}

/// The Spaces the grid shows: those whose name, words or tags hold the search text and whose status is the one chosen ("all" for
/// every one), in the order asked for. Ties keep the Worker's order.
pub fn shown(list: &[Place], query: &str, status: &str, sort: &str) -> Vec<Place> {
    let query = query.to_lowercase();
    let mut out: Vec<Place> = list
        .iter()
        .filter(|place| {
            let matches_search = query.is_empty()
                || place.name.to_lowercase().contains(&query)
                || place.description.to_lowercase().contains(&query)
                || query_matches_tag(&query, &place.tags);
            let matches_status = status == "all" || place.status.as_str() == status;
            matches_search && matches_status
        })
        .cloned()
        .collect();
    match sort {
        "name" => out.sort_by_key(|p| p.name.to_lowercase()),
        "visits" => out.sort_by(|a, b| b.visits.cmp(&a.visits)),
        "created" => out.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
        _ => out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at)),
    }
    out
}

/// The orders the list offers.
const SORTS: [(&str, &str); 4] = [("updated", "Last updated"), ("created", "Newest published"), ("name", "Name"), ("visits", "Most visits")];

/// The figures in the header, from the account's own Spaces: how many, their visits, how many are published, and how many are
/// in review when any are. Nothing at all when there are none.
pub fn figures_of(list: &[Place]) -> Vec<(&'static str, String)> {
    if list.is_empty() {
        return Vec::new();
    }
    let mut out = vec![
        ("Spaces", format_count(list.len() as u64)),
        ("Visits", format_count(list.iter().map(|p| p.visits).sum::<u64>())),
        ("Published", format_count(status_count(list, "published") as u64)),
    ];
    let review = status_count(list, "review");
    if review > 0 {
        out.push(("In review", format_count(review as u64)));
    }
    out
}

/// "Updated today", "Updated 3 days ago"; nothing when the Worker sent no time.
pub fn edited_words(iso: &str, now_ms: f64) -> Option<String> {
    published_words(iso.trim(), now_ms).map(|w| format!("Updated {}", w.trim_start_matches("published ")))
}

/// "Solo", or "Up to 8 players".
pub fn room_words(max_players: u32) -> String {
    if max_players <= 1 { "Solo".to_string() } else { format!("Up to {max_players} players") }
}

/// A card's picture: an https address or a path on this site, never a protocol-relative address.
pub fn card_art(url: Option<&str>) -> Option<String> {
    url.map(str::trim).filter(|u| u.starts_with("https://") || (u.starts_with('/') && !u.starts_with("//"))).map(str::to_string)
}

/// What makes a card draw again: a Space whose status, tags or last edit changed is a new card.
fn card_key(place: &Place) -> String {
    format!("{}|{}|{}|{}", place.id, place.status.as_str(), place.updated_at, place.tags.join(","))
}

// -----------------------------------------------------------------------------
// 2. The page
// -----------------------------------------------------------------------------

/// Projects page - manage all user's spaces and creations.
#[component]
pub fn ProjectsPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();

    // The project whose settings dialog is open, the one an Archive is waiting on, and the line that says what just happened.
    let settings_open = RwSignal::new(None::<Place>);
    let archive_ask = RwSignal::new(None::<Place>);
    let toast = RwSignal::new(None::<Toast>);
    // The project whose review panel is open. A review email links to
    // /projects?review=<id>, which opens it.
    let review_open = RwSignal::new(review_id_from_query(&current_query()));
    let panel_api = app_state.api_url.clone();

    // API data state
    let spaces = RwSignal::new(Load::<Vec<Place>>::Loading);

    // What each published project can sell, from the account's commerce record. A project with no entry here has
    // nothing to show: no chip and no Sell button. The record is asked for once per sign-in.
    let sellers = RwSignal::new(Vec::<SellerSimulation>::new());
    let auth = app_state.auth;
    let signed_in = Memo::new(move |_| matches!(auth.get(), AuthState::Authenticated(_)));
    {
        let api_url = app_state.api_url.clone();
        Effect::new(move |_| {
            if !signed_in.get() {
                sellers.set(Vec::new());
                return;
            }
            let api_url = api_url.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match get_commerce_account(&api_url, CommerceMode::Test).await {
                    Ok(account) => sellers.set(account.simulations),
                    Err(e) => log::warn!("Could not read what each project can sell: {}", commerce_error_text(&e)),
                }
            });
        });
    }
    let viewer = Signal::derive(move || match auth.get() {
        AuthState::Authenticated(_) => Viewer::SignedIn,
        AuthState::Unauthenticated => Viewer::SignedOut,
        AuthState::Unknown => Viewer::Unknown,
    });

    // Fetch projects from API on mount, and again after an archive or unarchive.
    let reload = {
        let api_url = app_state.api_url.clone();
        move || {
        let api_url = api_url.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&api_url);

            // Fetch user's projects
            match client.get::<ProjectsResponse>("/api/projects").await {
                Ok(response) => {
                    let places: Vec<Place> = response.projects.into_iter().map(|p| {
                        // The API reports the review state for anything not yet
                        // listed (see SpaceStatus::from_api).
                        let status = SpaceStatus::from_api(&p.status);
                        Place {
                            id: p.id,
                            name: p.name,
                            description: p.description.unwrap_or_default(),
                            thumbnail_url: p.thumbnail_url,
                            status,
                            visits: p.play_count,
                            favorites: p.favorite_count,
                            max_players: p.max_players,
                            tags: p.tags.unwrap_or_default(),
                            tags_source: p.tags_source,
                            updated_at: p.last_edited,
                            created_at: p.created_at,
                            is_public: p.is_public,
                        }
                    }).collect();
                    spaces.set(Load::Ready(places));
                }
                Err(e) => {
                    log::warn!("Failed to fetch projects: {:?}", e);
                    // A read that fails after a good one keeps what the good one showed.
                    if !matches!(spaces.get_untracked(), Load::Ready(_)) {
                        spaces.set(Load::Failed(crate::api::failure_sentence(&e)));
                    }
                }
            }
        });
        }
    };
    let first_load = reload.clone();
    Effect::new(move |_| first_load());
    let retry = {
        let reload = reload.clone();
        Callback::new(move |_| {
            spaces.set(Load::Loading);
            reload();
        })
    };

    // Archive and unarchive go through the Worker; the list is read again afterwards, so it shows what the Worker kept.
    let archive_api = app_state.api_url.clone();
    let set_archived = {
        let reload = reload.clone();
        move |place: Place, archive: bool| {
            let api_url = archive_api.clone();
            let reload = reload.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let client = ApiClient::new(&api_url);
                let route = if archive { "archive" } else { "unarchive" };
                let sent = client.post::<serde_json::Value, _>(&format!("/api/simulations/{}/{route}", place.id), &serde_json::json!({})).await;
                match sent {
                    Ok(_) => {
                        reload();
                        toast.set(Some(if archive {
                            Toast { message: format!("Archived {}. It is out of the Gallery, and only you can see it.", place.name), undo: Some(place.id.clone()), failed: false }
                        } else {
                            Toast { message: format!("{} is back in the Gallery, as it was.", place.name), undo: None, failed: false }
                        }));
                    }
                    Err(e) => toast.set(Some(Toast { message: format!("Could not {} {}: {}", route, place.name, crate::api::failure_sentence(&e)), undo: None, failed: true })),
                }
            });
        }
    };
    let archive_now = {
        let set_archived = set_archived.clone();
        Callback::new(move |place: Place| set_archived(place, true))
    };
    let unarchive_now = {
        let set_archived = set_archived.clone();
        Callback::new(move |place: Place| set_archived(place, false))
    };
    // A card's Archive asks first; Unarchive undoes it at once.
    let on_archive = Callback::new(move |place: Place| {
        if place.status == SpaceStatus::Archived {
            unarchive_now.run(place);
        } else {
            archive_ask.set(Some(place));
        }
    });
    let on_configure = Callback::new(move |place: Place| settings_open.set(Some(place)));
    // The tag editor saved: close the dialog, read the list again, and say that the Space goes back to review.
    let after_tags = {
        let reload = reload.clone();
        Callback::new(move |message: String| {
            settings_open.set(None);
            reload();
            toast.set(Some(Toast { message, undo: None, failed: false }));
        })
    };
    // "Undo" under the line that says it was archived: the listing is relisted by its id.
    let undo_archive = {
        let set_archived = set_archived.clone();
        move |id: String| {
            let place = Place { id, name: "the Space".to_string(), description: String::new(), thumbnail_url: None, status: SpaceStatus::Archived, visits: 0, favorites: 0, max_players: 0, tags: Vec::new(), tags_source: None, updated_at: String::new(), created_at: String::new(), is_public: false };
            set_archived(place, false);
        }
    };

    view! {
        <div class="page g-page pj-page">
            <CentralNav active="projects".to_string() />
            <div class="g-backdrop" aria-hidden="true"></div>
            <main class="g-main">
                <ProjectsBody spaces=spaces viewer=viewer review_open=review_open on_configure=on_configure on_archive=on_archive sellers=sellers retry=retry />
            </main>

            // A line that says what an archive just did, with the way back
            {move || toast.get().map(|t| {
                let undo = t.undo.clone();
                let undo_archive = undo_archive.clone();
                view! {
                    <div class=if t.failed { "projects-toast failed" } else { "projects-toast" } role=if t.failed { "alert" } else { "status" }>
                        <span>{t.message.clone()}</span>
                        {undo.map(|id| view! {
                            <button class="projects-toast-action" on:click=move |_| { let id = id.clone(); let undo_archive = undo_archive.clone(); after_event(move || { toast.set(None); undo_archive(id); }); }>"Undo"</button>
                        })}
                        <button class="projects-toast-close" aria-label="Dismiss" on:click=move |_| after_event(move || toast.set(None))>"x"</button>
                    </div>
                }
            })}

            // Archive asks first
            {move || archive_ask.get().map(|place| {
                let confirm_place = place.clone();
                view! {
                    <div class="modal-overlay" on:click=move |_| after_event(move || archive_ask.set(None))>
                        <div class="modal-content archive-dialog" role="alertdialog" aria-modal="true" aria-labelledby="archive-title" on:click=|e| e.stop_propagation()>
                            <h3 id="archive-title">{format!("Archive {}?", place.name)}</h3>
                            <p>"It leaves the Gallery and only you can see it. Its review, its world and its counts stay as they are. You can unarchive it any time, and the line that appears after this has an Undo."</p>
                            <div class="modal-actions">
                                <button class="btn btn-secondary" on:click=move |_| after_event(move || archive_ask.set(None))>"Cancel"</button>
                                <button class="btn btn-primary" on:click=move |_| { let p = confirm_place.clone(); after_event(move || { archive_ask.set(None); archive_now.run(p); }); }>"Archive"</button>
                            </div>
                        </div>
                    </div>
                }
            })}

            // The listing's settings: what they are now, and where each one is changed
            {move || settings_open.get().map(|place| {
                let studio = format!("eustress://edit/{}", place.id);
                let id = place.id.clone();
                let has_review = place.status.has_review();
                let archived = place.status == SpaceStatus::Archived;
                let place_for_archive = place.clone();
                let tags_now = place.tags.clone();
                let tags_source = place.tags_source.clone();
                let tags_name = place.name.clone();
                let tags_locked = place.status == SpaceStatus::UnderReview;
                view! {
                    <div class="modal-overlay" on:click=move |_| after_event(move || settings_open.set(None))>
                        <div class="modal-content settings-dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title" on:click=|e| e.stop_propagation()>
                            <h3 id="settings-title">{format!("Settings: {}", place.name)}</h3>
                            <dl class="settings-facts">
                                <dt>"Status"</dt><dd>{place.status.display_name()}</dd>
                                <dt>"Visibility"</dt><dd>{if place.is_public { "Public once approved" } else { "Private: only you" }}</dd>
                                <dt>"Max players"</dt><dd>{format!("{} (from the last publish)", place.max_players)}</dd>
                                <dt>"Description"</dt><dd>{if place.description.is_empty() { "None yet".to_string() } else { place.description.clone() }}</dd>
                            </dl>
                            <h4>"Tags"</h4>
                            <TagEditor place_id=id.clone() place_name=tags_name tags=tags_now tags_source=tags_source locked=tags_locked on_saved=after_tags />
                            <h4>"Where each one is changed"</h4>
                            <ul class="settings-where">
                                <li><strong>"Title and description"</strong>": the Publish dialog in Studio. Publishing again updates the listing, and a change to the words goes back to review."</li>
                                <li><strong>"Tags"</strong>": here, or in the Publish dialog in Studio. A change to the tags goes back to review."</li>
                                <li><strong>"Max players"</strong>": the Space's Players service in Studio (Properties, MaxPlayers). It reaches this page the next time you publish."</li>
                                <li><strong>"Visibility"</strong>": Archive and Unarchive here, or the public setting in the Publish dialog."</li>
                            </ul>
                            <div class="modal-actions">
                                <a href=studio class="btn btn-secondary">"Edit in Studio"</a>
                                {has_review.then(|| { let id = id.clone(); view! { <button class="btn btn-secondary" on:click=move |_| { let id = id.clone(); after_event(move || { settings_open.set(None); review_open.set(Some(id)); }); }>"Open the review"</button> } })}
                                <button class="btn btn-secondary" on:click=move |_| { let p = place_for_archive.clone(); after_event(move || { settings_open.set(None); on_archive.run(p); }); }>{if archived { "Unarchive" } else { "Archive" }}</button>
                                <button class="btn btn-primary" on:click=move |_| after_event(move || settings_open.set(None))>"Close"</button>
                            </div>
                        </div>
                    </div>
                }
            })}

            // What review decided about a project, and the appeal
            {move || review_open.get().map(|id| view! {
                <ReviewPanel
                    sim_id=id
                    api_url=panel_api.clone()
                    on_close=move || review_open.set(None)
                />
            })}

            <Footer />
        </div>
    }
}

/// The query string of the page in front of the visitor. The prerender has no
/// page.
#[cfg(not(feature = "ssr"))]
fn current_query() -> String {
    web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
}

#[cfg(feature = "ssr")]
fn current_query() -> String {
    String::new()
}

/// The project id in `?review=<id>`. Only a plausible id passes, since it goes
/// into an API path: lowercase hex and dashes, 1 to 64 characters.
fn review_id_from_query(search: &str) -> Option<String> {
    search
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == "review")
        .map(|(_, value)| value)
        .filter(|id| !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-'))
        .map(str::to_string)
}

/// What the page says after an archive or unarchive.
#[derive(Clone, Debug, PartialEq)]
struct Toast {
    message: String,
    /// The listing to relist when "Undo" is pressed.
    undo: Option<String>,
    failed: bool,
}

// -----------------------------------------------------------------------------
// 3. The body: header, toolbar, grid and states
// -----------------------------------------------------------------------------

/// Everything under the top bar except the dialogs: the header with its figures, the account's Spaces in their state, and the
/// band about publishing.
#[component]
pub fn ProjectsBody(
    spaces: RwSignal<Load<Vec<Place>>>,
    viewer: Signal<Viewer>,
    review_open: RwSignal<Option<String>>,
    on_configure: Callback<Place>,
    on_archive: Callback<Place>,
    sellers: RwSignal<Vec<SellerSimulation>>,
    retry: Callback<()>,
) -> impl IntoView {
    view! {
        <div class="g-wrap pj-wrap">
            <header class="g-glass pj-hero" aria-labelledby="pj-title">
                <div class="pj-hero-copy">
                    <div class="g-kicker"><span class="g-kick-rule"></span>"Projects"</div>
                    <h1 id="pj-title" class="pj-title">"Your projects"</h1>
                    <p class="pj-lede">"Every Space you publish from Eustress Studio: where it stands in review, its tags, who can see it, and what it sells."</p>
                    <SellNote />
                </div>
                {move || {
                    let figures = match (viewer.get(), spaces.get()) {
                        (Viewer::SignedIn, Load::Ready(list)) => figures_of(&list),
                        _ => Vec::new(),
                    };
                    (!figures.is_empty()).then(|| view! {
                        <dl class="pj-figures">
                            {figures.into_iter().map(|(k, v)| view! { <div><dt>{k}</dt><dd>{v}</dd></div> }).collect_view()}
                        </dl>
                    })
                }}
            </header>

            <section class="g-section pj-section" aria-labelledby="pj-list-h">
                <div class="g-head"><h2 id="pj-list-h">"Your Spaces"</h2><span class="g-rule"></span></div>
                {move || match (viewer.get(), spaces.get()) {
                    (Viewer::SignedOut, _) => signed_out_state().into_any(),
                    (Viewer::Unknown, _) | (_, Load::Loading) => loading_cards().into_any(),
                    (_, Load::Failed(why)) => failed_state(why, retry).into_any(),
                    (_, Load::Ready(list)) if list.is_empty() => empty_state().into_any(),
                    (_, Load::Ready(list)) => view! {
                        <ProjectList list=list review_open=review_open on_configure=on_configure on_archive=on_archive sellers=sellers />
                    }.into_any(),
                }}
            </section>

            <div class="g-glass g-band pj-band">
                <div>
                    <h2>"Publish from Eustress Studio"</h2>
                    <p>"A Space you publish shows here at once, with its review status, and in the Gallery once review approves it. API keys for DataStoreService, HttpService and selling are on the AI page."</p>
                </div>
                <div class="g-actions">
                    <a class="g-btn g-btn-primary g-btn-lg" href="/download">"Download Studio"</a>
                    <a class="g-btn g-btn-lg" href="/docs/publishing">"How publishing works"</a>
                    <a class="g-btn g-btn-lg" href="/ai">"API keys"</a>
                </div>
            </div>
        </div>
    }
}

const SEARCH_GLYPH: &str = "M11 4a7 7 0 1 0 0 14a7 7 0 0 0 0-14zM20 20l-3.6-3.6";

/// The toolbar (search, status, order, layout) and the grid of the Spaces that pass it.
#[component]
fn ProjectList(
    list: Vec<Place>,
    review_open: RwSignal<Option<String>>,
    on_configure: Callback<Place>,
    on_archive: Callback<Place>,
    sellers: RwSignal<Vec<SellerSimulation>>,
) -> impl IntoView {
    let all = StoredValue::new(list);
    let search_query = RwSignal::new(String::new());
    let filter_status = RwSignal::new("all".to_string());
    let sort_by = RwSignal::new("updated".to_string());
    let view_mode = RwSignal::new("grid".to_string()); // "grid" or "list"
    let filtered_spaces = move || all.with_value(|l| shown(l, &search_query.get(), &filter_status.get(), &sort_by.get()));
    // The grid is built once per change between "something" and "nothing"; the For inside keeps each card in step.
    let nothing = Memo::new(move |_| filtered_spaces().is_empty());
    let chips = all.with_value(|l| status_chips(l));

    view! {
        <div class="pj-toolbar">
            <form class="pj-search" role="search" on:submit=move |ev: leptos::ev::SubmitEvent| ev.prevent_default()>
                <label class="g-sr" for="pj-q">"Search your Spaces by name, words or tag"</label>
                <svg class="pj-search-icon" viewBox="0 0 24 24" aria-hidden="true"><path d=SEARCH_GLYPH/></svg>
                <input id="pj-q" class="pj-q" type="search" placeholder="Search your Spaces" autocomplete="off" spellcheck="false"
                    prop:value=move || search_query.get()
                    on:input=move |e| search_query.set(event_target_value(&e)) />
            </form>
            <div class="g-chips pj-filters" role="group" aria-label="Show">
                {chips.into_iter().map(|(key, label, n)| view! {
                    <button type="button" class="g-chip g-chip-btn" class:on=move || filter_status.get() == key aria-pressed=move || (filter_status.get() == key).to_string()
                        on:click=move |_| filter_status.set(key.to_string())>
                        {label}<span class="pj-n">{n}</span>
                    </button>
                }).collect_view()}
            </div>
            <div class="pj-tools">
                <label class="pj-sort">
                    <span class="g-sr">"Order"</span>
                    <select prop:value=move || sort_by.get() on:change=move |e| sort_by.set(event_target_value(&e))>
                        {SORTS.into_iter().map(|(key, label)| view! { <option value=key selected=move || sort_by.get() == key>{label}</option> }).collect_view()}
                    </select>
                </label>
                <div class="pj-view" role="group" aria-label="Layout">
                    <button type="button" class="pj-view-btn" aria-pressed=move || (view_mode.get() == "grid").to_string() on:click=move |_| view_mode.set("grid".to_string()) title="Grid">
                        <img src="/assets/icons/grid.svg" alt="Grid" />
                    </button>
                    <button type="button" class="pj-view-btn" aria-pressed=move || (view_mode.get() == "list").to_string() on:click=move |_| view_mode.set("list".to_string()) title="List">
                        <img src="/assets/icons/list.svg" alt="List" />
                    </button>
                </div>
            </div>
        </div>
        {move || {
            if nothing.get() {
                let q = search_query.get();
                let hint = if q.trim().is_empty() { "None of your Spaces has this status.".to_string() } else { format!("None of your Spaces matches \u{201c}{}\u{201d}.", q.trim()) };
                view! {
                    <div class="g-glass g-none pj-none">
                        <b>"No Space matches"</b>
                        <p>{hint}</p>
                        <div class="g-actions">
                            <button type="button" class="g-btn" on:click=move |_| { search_query.set(String::new()); filter_status.set("all".to_string()); }>"Show all"</button>
                        </div>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="pj-grid" class:pj-list=move || view_mode.get() == "list">
                        <For
                            each=filtered_spaces
                            key=card_key
                            children=move |place| {
                                view! { <PlaceCard place=place review_open=review_open on_configure=on_configure on_archive=on_archive sellers=sellers /> }
                            }
                        />
                    </div>
                }.into_any()
            }
        }}
    }
}

// -----------------------------------------------------------------------------
// 4. A project's card
// -----------------------------------------------------------------------------

/// A card's picture, or the cube when the Space has none. The Worker answers 404 for a capture it will not serve (animated,
/// transparent, several images, not an image, too large), so a picture that fails to load clears art_ok and the card draws the
/// same cube as a Space with no capture, never the browser's broken-image icon. Each card makes its own signal.
fn card_picture(thumbnail: Option<String>, art_ok: RwSignal<bool>) -> impl IntoView {
    move || match thumbnail.clone().filter(|_| art_ok.get()) {
        Some(src) => view! { <img src=src alt="" class="pj-img" loading="lazy" decoding="async" on:error=move |_| art_ok.set(false) /> }.into_any(),
        None => view! { <span class="pj-art-empty" aria-hidden="true"><img src="/assets/icons/cube.svg" alt="" /></span> }.into_any(),
    }
}

/// A Space of the account's, built from the Gallery's card: its capture, its status, its name and words, its tags (each a link
/// to the Gallery's filter for that tag), what it sells, its visits, its room and its last edit, and what can be done with it.
#[component]
fn PlaceCard(
    place: Place,
    review_open: RwSignal<Option<String>>,
    on_configure: Callback<Place>,
    on_archive: Callback<Place>,
    sellers: RwSignal<Vec<SellerSimulation>>,
) -> impl IntoView {
    let playable = matches!(place.status, SpaceStatus::Published | SpaceStatus::Private);
    // This page lists the signed-in person's own Spaces, so the reader is always the author: tags the review chose say so.
    let card_tags = place.tags.clone();
    let tags_note = suggested_label(&place.tags, place.tags_source.as_deref(), true);
    let archived = place.status == SpaceStatus::Archived;
    let has_review = place.status.has_review();
    // Play opens the Play chooser in place, like every Play button on the site.
    let chooser = expect_context::<crate::components::play_chooser::PlayChooserSlot>();
    let play_id = place.id.clone();
    // Edit opens Studio through its own link; Studio downloads the listing's world and opens it.
    let edit_url = format!("eustress://edit/{}", place.id);
    let status_class = format!("g-chip g-badge pj-status {}", place.status.as_str());
    let thumbnail = card_art(place.thumbnail_url.as_deref());
    // A listed Space's name opens its page in the Gallery.
    let public_href = (place.status == SpaceStatus::Published).then(|| format!("/simulation/{}", urlencoding::encode(&place.id)));
    let edited = edited_words(&place.updated_at, now_ms());
    let room = room_words(place.max_players);
    let visits = place.visits;
    // This project's selling entry, once the account's commerce record has loaded. No entry: no chip and no Sell button.
    let seller_id = place.id.clone();
    let seller = Memo::new(move |_| sellers.with(|list| seller_for(list, &seller_id).cloned()));

    // Every item does something. Play only where the listing can be played; Review only where review has something to say.
    let mut items = Vec::new();
    if playable {
        let id = play_id.clone();
        items.push(Item::new("Play", "play.svg", Action::Call(Callback::new(move |_| chooser.open(&id)))));
    }
    items.push(Item::new("Edit in Studio", "edit.svg", Action::Open(edit_url.clone())));
    if has_review {
        let id = place.id.clone();
        items.push(Item::new("Review", "shield.svg", Action::Call(Callback::new(move |_| review_open.set(Some(id.clone()))))));
    }
    let for_settings = place.clone();
    items.push(Item::new("Settings", "settings.svg", Action::Call(Callback::new(move |_| on_configure.run(for_settings.clone())))));
    let for_archive = place.clone();
    items.push(
        Item::new(if archived { "Unarchive" } else { "Archive" }, "archive.svg", Action::Call(Callback::new(move |_| on_archive.run(for_archive.clone()))))
            .divided(),
    );

    view! {
        <article class="g-card pj-card">
            <div class="g-media">
                {card_picture(thumbnail, RwSignal::new(true))}
                <span class=status_class>{place.status.display_name()}</span>
            </div>

            <div class="g-body pj-body">
                <div class="pj-head">
                    <h3 class="g-name pj-name">
                        {match public_href {
                            Some(href) => view! { <a class="pj-link" href=href>{place.name.clone()}</a> }.into_any(),
                            None => view! { <span>{place.name.clone()}</span> }.into_any(),
                        }}
                    </h3>
                    <div class="menu-container">
                        <ActionMenu label=format!("Options for {}", place.name) items=items />
                    </div>
                </div>

                {(!place.description.trim().is_empty()).then(|| view! { <p class="pj-desc">{place.description.clone()}</p> })}

                <div class="g-chips pj-tags">
                    {if card_tags.is_empty() {
                        view! { <span class="pj-note">"No tags yet"</span> }.into_any()
                    } else {
                        card_tags.iter().map(|t| {
                            let href = gallery_href(std::slice::from_ref(t));
                            view! { <a class="g-chip g-tag" href=href>{t.clone()}</a> }
                        }).collect_view().into_any()
                    }}
                    {tags_note.map(|words| view! { <span class="pj-note">{words}</span> })}
                </div>

                {move || seller.get().map(|sim| view! { <SellChip sim=sim /> })}

                <div class="g-facts pj-facts">
                    <span><b>{format_count(visits)}</b>{if visits == 1 { " visit" } else { " visits" }}</span>
                    // The number is the one the last publish stored, so it can lag until the next publish from Studio.
                    <span title="From the last publish. It updates the next time you publish from Studio.">{room}</span>
                    {edited.map(|w| view! { <span>{w}</span> })}
                </div>

                <div class="card-actions pj-actions">
                    {playable.then(|| view! {
                        <button type="button" class="action-btn primary" on:click=move |_| chooser.open(&play_id)>
                            <img src="/assets/icons/play.svg" alt="" />
                            "Play"
                        </button>
                    })}
                    <a href=edit_url.clone() class="action-btn secondary">
                        <img src="/assets/icons/edit.svg" alt="" />
                        "Edit"
                    </a>
                    {has_review.then(|| {
                        let id = place.id.clone();
                        view! {
                            <button class="action-btn secondary" on:click=move |_| review_open.set(Some(id.clone()))>
                                <img src="/assets/icons/shield.svg" alt="" />
                                "Review"
                            </button>
                        }
                    })}
                    {move || seller.get().map(|sim| view! { <SellButton sim_id=sim.id /> })}
                </div>
            </div>
        </article>
    }
}

// -----------------------------------------------------------------------------
// 5. Loading, failed, signed-out and empty states
// -----------------------------------------------------------------------------

const EMPTY_GLYPH: &str = "M12 3a9 9 0 1 0 0 18a9 9 0 0 0 0-18zM8 12h8";
const FAILED_GLYPH: &str = "M12 3a9 9 0 1 0 0 18a9 9 0 0 0 0-18zM12 7.5v5.5M12 16.5v.01";
const KEY_GLYPH: &str = "M7 14a4 4 0 1 1 3.5-6h9.5v4h-2v2h-3v-2h-4.5A4 4 0 0 1 7 14z";

/// The shape of the grid while the Spaces load, not made-up cards.
fn loading_cards() -> impl IntoView {
    view! {
        <div class="pj-grid" role="status" aria-busy="true" aria-label="Loading your Spaces">
            {(0..3).map(|_| view! { <div class="g-card g-skel-card"><div class="g-skel g-skel-media"></div><div class="g-body"><span class="g-skel w70"></span><span class="g-skel w40"></span></div></div> }).collect_view()}
        </div>
    }
}

/// The read did not answer: said plainly, never shown as an account with nothing in it.
fn failed_state(why: String, retry: Callback<()>) -> impl IntoView {
    view! {
        <div class="g-glass pj-state pj-failed" role="alert">
            <span class="g-glyph" aria-hidden="true"><svg viewBox="0 0 24 24"><path d=FAILED_GLYPH/></svg></span>
            <div>
                <b>"Your Spaces could not load"</b>
                <p>{why}</p>
                <button type="button" class="g-btn g-btn-sm" on:click=move |_| retry.run(())>"Try again"</button>
            </div>
        </div>
    }
}

/// Nobody is signed in: the Worker has nobody's Spaces to send.
fn signed_out_state() -> impl IntoView {
    view! {
        <div class="g-glass pj-state pj-signed-out">
            <span class="g-glyph" aria-hidden="true"><svg viewBox="0 0 24 24"><path d=KEY_GLYPH/></svg></span>
            <div>
                <b>"Sign in to see your projects"</b>
                <p>"This page lists the Spaces your account published from Eustress Studio. Sign in with that account."</p>
                <a class="g-btn g-btn-primary g-btn-sm" href="/login">"Sign in"</a>
            </div>
        </div>
    }
}

/// Signed in, and nothing published yet.
fn empty_state() -> impl IntoView {
    view! {
        <div class="g-glass pj-state pj-empty">
            <span class="g-glyph" aria-hidden="true"><svg viewBox="0 0 24 24"><path d=EMPTY_GLYPH/></svg></span>
            <div>
                <b>"You have not published a Space yet"</b>
                <p>"Build one in Eustress Studio and publish it. It shows here right away, with where it stands in review."</p>
                <div class="g-actions">
                    <a class="g-btn g-btn-primary g-btn-sm" href="/download">"Download Studio"</a>
                    <a class="g-btn g-btn-sm" href="/docs/publishing">"How publishing works"</a>
                </div>
            </div>
        </div>
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// "1.2K": the API keys table's usage figure.
fn format_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

// -----------------------------------------------------------------------------
// 6. API keys (drawn by /ai)
// -----------------------------------------------------------------------------

/// API key data from backend
#[derive(Clone, Debug, serde::Deserialize)]
pub struct ApiKeyData {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub key_type: String,
    /// "test" or "live", for a commerce key.
    #[serde(default)]
    pub mode: String,
    pub created_at: String,
    pub last_used: Option<String>,
    pub usage_count: u64,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct ApiKeysResponse {
    pub keys: Vec<ApiKeyData>,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct CreateKeyResponse {
    pub id: String,
    pub name: String,
    pub key: String,
    pub key_type: String,
}

/// API Keys management section for signed-in users.
#[component]
pub fn ApiKeysSection() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    
    // State
    let api_keys = RwSignal::new(Vec::<ApiKeyData>::new());
    let is_loading = RwSignal::new(true);
    let show_create_modal = RwSignal::new(false);
    let new_key_name = RwSignal::new(String::new());
    let new_key_type = RwSignal::new("datastore".to_string());
    // A commerce key is a test key or a live key, as in `eustress commerce`.
    let new_key_mode = RwSignal::new("test".to_string());
    let newly_created_key = RwSignal::new(None::<String>);
    let copied_key_id = RwSignal::new(None::<String>);
    let key_error = RwSignal::new(None::<String>);
    // A passkey session cannot create keys (revoking stays on).
    let auth = app_state.auth;
    let passkey_session = move || auth.with(|a| crate::pages::passkeys::key_change_blocked(a.user()));
    
    // Fetch API keys on mount
    let api_url = app_state.api_url.clone();
    Effect::new(move |_| {
        let api_url = api_url.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&api_url);
            match client.get::<ApiKeysResponse>("/api/keys").await {
                Ok(response) => {
                    api_keys.set(response.keys);
                }
                Err(e) => {
                    log::warn!("Failed to fetch API keys: {:?}", e);
                }
            }
            is_loading.set(false);
        });
    });
    
    // Create new key handler
    let api_url_stored = StoredValue::new(app_state.api_url.clone());
    let create_key = move |_| {
        let api_url = api_url_stored.get_value();
        let name = new_key_name.get();
        let key_type = new_key_type.get();
        let mode = new_key_mode.get();

        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&api_url);
            let body = serde_json::json!({
                "name": name,
                "key_type": key_type,
                "mode": mode
            });

            match client.post::<CreateKeyResponse, _>("/api/keys", &body).await {
                Ok(response) => {
                    newly_created_key.set(Some(response.key.clone()));
                    key_error.set(None);
                    // Refresh keys list
                    if let Ok(keys_response) = client.get::<ApiKeysResponse>("/api/keys").await {
                        api_keys.set(keys_response.keys);
                    }
                }
                Err(e) => {
                    log::error!("Failed to create API key: {:?}", e);
                    key_error.set(Some(crate::pages::passkeys::identity_file_required_words(&e).unwrap_or_else(|| format!("The key was not created: {e}"))));
                }
            }
        });
    };

    // Revoke: the key stops working at once, everywhere it is used.
    let revoke_key = move |key_id: String, key_name: String| {
        let sure = web_sys::window()
            .and_then(|w| w.confirm_with_message(&format!("Revoke \u{201c}{key_name}\u{201d}? Anything using it stops working.")).ok())
            .unwrap_or(false);
        if !sure {
            return;
        }
        let api_url = api_url_stored.get_value();
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&api_url);
            match client.delete::<serde_json::Value>(&format!("/api/keys/{key_id}")).await {
                Ok(_) => {
                    api_keys.update(|keys| keys.retain(|k| k.id != key_id));
                    key_error.set(None);
                }
                Err(e) => key_error.set(Some(format!("The key was not revoked: {e}"))),
            }
        });
    };
    
    // Copy key to clipboard via navigator.clipboard JS API
    let copy_to_clipboard = move |key: String, key_id: String| {
        let js_code = format!(
            "navigator.clipboard.writeText('{}')",
            key.replace('\'', "\\'")
        );
        let _ = js_sys::eval(&js_code);

        copied_key_id.set(Some(key_id));
        let copied_signal = copied_key_id;
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(2000).await;
            copied_signal.set(None);
        });
    };
    
    view! {
        <section class="api-keys-section">
            <div class="section-header">
                <div class="header-content">
                    <svg class="section-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4"/>
                    </svg>
                    <div>
                        <h2>"API Keys"</h2>
                        <p>"Manage your API keys for DataStoreService, HttpService, AI Training, and selling (eustress commerce)"</p>
                    </div>
                </div>
                <button 
                    class="btn-create-key"
                    disabled=move || passkey_session()
                    on:click=move |_| {
                        new_key_name.set(String::new());
                        newly_created_key.set(None);
                        show_create_modal.set(true);
                    }
                >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <line x1="12" y1="5" x2="12" y2="19"/>
                        <line x1="5" y1="12" x2="19" y2="12"/>
                    </svg>
                    "Create Key"
                </button>
            </div>
            
            {move || passkey_session().then(|| view! { <p class="keys-note" role="status">{crate::pages::passkeys::EMAIL_AND_KEYS_SENTENCE}</p> })}
            {move || key_error.get().map(|e| view! { <p class="keys-error" role="alert">{e}</p> })}

            <Show when=move || is_loading.get()>
                <div class="keys-loading">
                    <div class="spinner"></div>
                    "Loading API keys..."
                </div>
            </Show>
            
            <Show when=move || !is_loading.get() && api_keys.get().is_empty()>
                <div class="keys-empty">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4"/>
                    </svg>
                    <p>"No API keys yet. Create one to use DataStoreService or HttpService in your scripts."</p>
                </div>
            </Show>
            
            <Show when=move || !is_loading.get() && !api_keys.get().is_empty()>
                <div class="keys-table">
                    <div class="keys-header">
                        <span class="col-name">"Name"</span>
                        <span class="col-type">"Type"</span>
                        <span class="col-key">"Key"</span>
                        <span class="col-usage">"Usage"</span>
                        <span class="col-created">"Created"</span>
                        <span class="col-actions">"Actions"</span>
                    </div>
                    <For
                        each=move || api_keys.get()
                        key=|key| key.id.clone()
                        children=move |key| {
                            let key_prefix = key.key_prefix.clone();
                            let on_revoke = {
                                let (id, name) = (key.id.clone(), key.name.clone());
                                move |_| revoke_key(id.clone(), name.clone())
                            };
                            let type_label = if key.key_type == "commerce" && !key.mode.is_empty() {
                                format!("commerce \u{00b7} {}", key.mode)
                            } else {
                                key.key_type.clone()
                            };
                            view! {
                                <div class="key-row">
                                    <span class="col-name">{key.name.clone()}</span>
                                    <span class="col-type">
                                        <span class="key-type-badge" class:datastore=key.key_type == "datastore" class:http=key.key_type == "http" class:ai=key.key_type == "ai" class:commerce=key.key_type == "commerce">
                                            {type_label}
                                        </span>
                                    </span>
                                    <span class="col-key">
                                        <code class="key-preview">{format!("{}...", key_prefix)}</code>
                                    </span>
                                    <span class="col-usage">{format_number(key.usage_count)}</span>
                                    <span class="col-created">{key.created_at.clone()}</span>
                                    <span class="col-actions">
                                        <button
                                            class="action-icon"
                                            title="Revoke Key"
                                            on:click=on_revoke
                                        >
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                                <polyline points="3 6 5 6 21 6"/>
                                                <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
                                            </svg>
                                        </button>
                                    </span>
                                </div>
                            }
                        }
                    />
                </div>
            </Show>
            
            // Create Key Modal
            <Show when=move || show_create_modal.get()>
                <div class="modal-overlay" on:click=move |_| crate::utils::after_event(move || show_create_modal.set(false))>
                    <div class="modal-content" on:click=|e| e.stop_propagation()>
                        <div class="modal-header">
                            <h3>"Create API Key"</h3>
                            <button class="modal-close" on:click=move |_| crate::utils::after_event(move || show_create_modal.set(false))>
                                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                    <line x1="18" y1="6" x2="6" y2="18"/>
                                    <line x1="6" y1="6" x2="18" y2="18"/>
                                </svg>
                            </button>
                        </div>
                        
                        <Show when=move || newly_created_key.get().is_some()>
                            <div class="key-created-success">
                                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                    <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/>
                                    <polyline points="22 4 12 14.01 9 11.01"/>
                                </svg>
                                <p>"Key created! Copy it now — you won't see it again."</p>
                                <div class="key-display">
                                    <code>{move || newly_created_key.get().unwrap_or_default()}</code>
                                    <button 
                                        class="copy-btn"
                                        on:click=move |_| {
                                            if let Some(key) = newly_created_key.get() {
                                                copy_to_clipboard(key, "new".to_string());
                                            }
                                        }
                                    >
                                        <Show 
                                            when=move || copied_key_id.get() == Some("new".to_string())
                                            fallback=|| view! {
                                                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                                    <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>
                                                    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
                                                </svg>
                                            }
                                        >
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                                <polyline points="20 6 9 17 4 12"/>
                                            </svg>
                                        </Show>
                                    </button>
                                </div>
                                <button class="btn-done" on:click=move |_| crate::utils::after_event(move || show_create_modal.set(false))>
                                    "Done"
                                </button>
                            </div>
                        </Show>
                        
                        <Show when=move || newly_created_key.get().is_none()>
                            <div class="modal-body">
                                <div class="form-group">
                                    <label>"Key Name"</label>
                                    <input 
                                        type="text"
                                        placeholder="e.g., Production DataStore"
                                        prop:value=move || new_key_name.get()
                                        on:input=move |e| new_key_name.set(event_target_value(&e))
                                    />
                                </div>
                                
                                <div class="form-group">
                                    <label>"Key Type"</label>
                                    <div class="key-type-options">
                                        <button 
                                            class="type-option"
                                            class:selected=move || new_key_type.get() == "datastore"
                                            on:click=move |_| new_key_type.set("datastore".to_string())
                                        >
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                                <ellipse cx="12" cy="5" rx="9" ry="3"/>
                                                <path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3"/>
                                                <path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"/>
                                            </svg>
                                            <span>"DataStore"</span>
                                            <small>"AWS DynamoDB persistence"</small>
                                        </button>
                                        <button 
                                            class="type-option"
                                            class:selected=move || new_key_type.get() == "http"
                                            on:click=move |_| new_key_type.set("http".to_string())
                                        >
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                                <circle cx="12" cy="12" r="10"/>
                                                <line x1="2" y1="12" x2="22" y2="12"/>
                                                <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/>
                                            </svg>
                                            <span>"HttpService"</span>
                                            <small>"External API requests"</small>
                                        </button>
                                        <button 
                                            class="type-option"
                                            class:selected=move || new_key_type.get() == "ai"
                                            on:click=move |_| new_key_type.set("ai".to_string())
                                        >
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                                <path d="M12 2L2 7l10 5 10-5-10-5z"/>
                                                <path d="M2 17l10 5 10-5"/>
                                                <path d="M2 12l10 5 10-5"/>
                                            </svg>
                                            <span>"AI Training"</span>
                                            <small>"Spatial data export"</small>
                                        </button>
                                        <button
                                            class="type-option"
                                            class:selected=move || new_key_type.get() == "commerce"
                                            on:click=move |_| new_key_type.set("commerce".to_string())
                                        >
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                                <path d="M12 2H2v10l9.29 9.29a1 1 0 0 0 1.41 0l8.59-8.59a1 1 0 0 0 0-1.41Z"/>
                                                <circle cx="7" cy="7" r="1.5"/>
                                            </svg>
                                            <span>"Commerce"</span>
                                            <small>"Sell products (eustress commerce)"</small>
                                        </button>
                                    </div>
                                </div>

                                <Show when=move || new_key_type.get() == "commerce">
                                    <div class="form-group">
                                        <label>"Mode"</label>
                                        <div class="key-type-options">
                                            <button
                                                class="type-option"
                                                class:selected=move || new_key_mode.get() == "test"
                                                on:click=move |_| new_key_mode.set("test".to_string())
                                            >
                                                <span>"Test"</span>
                                                <small>"Drafts and test purchases; moves no Tickets"</small>
                                            </button>
                                            <button
                                                class="type-option"
                                                class:selected=move || new_key_mode.get() == "live"
                                                on:click=move |_| new_key_mode.set("live".to_string())
                                            >
                                                <span>"Live"</span>
                                                <small>"Real sales, prices and refunds"</small>
                                            </button>
                                        </div>
                                    </div>
                                </Show>
                            </div>
                            
                            <div class="modal-footer">
                                <button class="btn-cancel" on:click=move |_| crate::utils::after_event(move || show_create_modal.set(false))>
                                    "Cancel"
                                </button>
                                <button 
                                    class="btn-create"
                                    disabled=move || new_key_name.get().is_empty()
                                    on:click=create_key
                                >
                                    "Create Key"
                                </button>
                            </div>
                        </Show>
                    </div>
                </div>
            </Show>
        </section>
    }
}

// -----------------------------------------------------------------------------
// 7. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = include_str!("projects.rs");
    const CSS: &str = include_str!("../../style/projects.css");
    const SELL_CSS: &str = include_str!("../../style/projects_sell.css");
    const TAGS_CSS: &str = include_str!("../../style/project_tags.css");
    const MAIN_CSS: &str = include_str!("../../style/main.css");
    const INDEX: &str = include_str!("../../index.html");

    /// The page's code, without these tests.
    fn source() -> &'static str {
        FILE.split(concat!("#[cfg", "(test)]")).next().unwrap()
    }

    /// The page's own code: everything before the API keys section, which the AI page draws.
    fn page_source() -> &'static str {
        let s = source();
        &s[..s.find("/// API key data from backend").expect("the API keys section")]
    }

    fn place(id: &str, name: &str, status: SpaceStatus, visits: u64, tags: &[&str]) -> Place {
        Place {
            id: id.to_string(),
            name: name.to_string(),
            description: format!("What {name} is."),
            thumbnail_url: None,
            status,
            visits,
            favorites: 7,
            max_players: 8,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            tags_source: None,
            updated_at: "2026-10-12T09:00:00Z".to_string(),
            created_at: "2026-09-01T09:00:00Z".to_string(),
            is_public: true,
        }
    }

    fn sample() -> Vec<Place> {
        let mut a = place("a1", "Harbor Crane", SpaceStatus::Published, 1204, &["physics", "co-op"]);
        a.thumbnail_url = Some("/assets/home/hero-city.webp".to_string());
        let mut b = place("b2", "Grid Load Lab", SpaceStatus::UnderReview, 3, &["city"]);
        b.tags_source = Some("judge".to_string());
        b.thumbnail_url = Some("//evil.example/x.png".to_string());
        let c = place("c3", "Old Bridge", SpaceStatus::Archived, 1, &[]);
        vec![a, b, c]
    }

    fn sim(id: &str) -> SellerSimulation {
        SellerSimulation { id: id.to_string(), name: "Harbor Crane".to_string(), spaces: Vec::new(), listed: true, can_sell: true, reason: None, products: Some(2), updated_at: None }
    }

    /// The body, rendered on the server the way the prerender renders a page, inside a router (a card's menu needs one).
    fn render(viewer: Viewer, spaces: Load<Vec<Place>>) -> String {
        use leptos::tachys::view::RenderHtml;
        use leptos_router::components::Router;
        use leptos_router::location::RequestUrl;
        let owner = Owner::new();
        owner.with(|| {
            provide_context(RequestUrl::new("/projects"));
            crate::components::play_chooser::provide_play_chooser();
            let spaces = RwSignal::new(spaces);
            let sellers = RwSignal::new(vec![sim("a1")]);
            let review_open = RwSignal::new(None::<String>);
            view! {
                <Router>
                    <ProjectsBody spaces=spaces viewer=Signal::stored(viewer) review_open=review_open on_configure=Callback::new(|_| ()) on_archive=Callback::new(|_| ()) sellers=sellers retry=Callback::new(|_| ()) />
                </Router>
            }
            .to_html()
        })
    }

    /// What the old page showed that no route said.
    const STAND_INS: [&str; 10] = [
        concat!("Eustress Engine ", "Protocol"),
        concat!("Team ", "Aggregation"),
        concat!("PII ", "redaction"),
        concat!("msg", "/s"),
        concat!("Organization-scoped ", "dataset"),
        concat!("Your ", "Creations"),
        concat!("heart", ".svg"),
        concat!("Favor", "ites"),
        concat!("total_", "favorites"),
        concat!("page-projects-", "industrial"),
    ];

    #[test]
    fn nothing_the_old_page_invented_comes_back() {
        for gone in STAND_INS {
            assert!(!page_source().contains(gone), "projects.rs shows {gone:?}");
        }
        for (what, html) in [
            ("full", render(Viewer::SignedIn, Load::Ready(sample()))),
            ("empty", render(Viewer::SignedIn, Load::Ready(vec![]))),
            ("signed out", render(Viewer::SignedOut, Load::Ready(vec![]))),
        ] {
            for gone in STAND_INS {
                assert!(!html.contains(gone), "the {what} page shows {gone:?}");
            }
        }
        // The Worker fixes favorite_count at 0: the page reads the field and never shows it.
        assert!(!page_source().contains(".favorites)") && !page_source().contains("p.favorites"), "favorites are not summed or shown");
        assert!(!render(Viewer::SignedIn, Load::Ready(sample())).contains(">7<"), "a card's favorites figure is not drawn");
    }

    #[test]
    fn signed_out_the_page_asks_for_a_sign_in_and_never_says_there_is_nothing() {
        // The Worker answers a signed-out read with an empty list; the page does not read that as "nothing published".
        let html = render(Viewer::SignedOut, Load::Ready(vec![]));
        assert!(html.contains("Sign in to see your projects") && html.contains("href=\"/login\""), "{html}");
        for claim in ["You have not published", "pj-figures", "<dt>", "class=\"g-card"] {
            assert!(!html.contains(claim), "signed out, the page claims {claim}");
        }
    }

    #[test]
    fn each_state_says_what_it_is() {
        let empty = render(Viewer::SignedIn, Load::Ready(vec![]));
        assert!(empty.contains("You have not published a Space yet") && empty.contains("href=\"/download\""), "{empty}");
        assert_eq!(empty.matches("class=\"g-glass pj-state pj-empty\"").count(), 1);
        assert!(!empty.contains("<dt>") && !empty.contains("pj-toolbar"), "nothing is drawn for nothing");
        let failed = render(Viewer::SignedIn, Load::Failed("Could not reach Eustress. Check your connection and try again.".to_string()));
        assert!(failed.contains("Your Spaces could not load") && failed.contains("Could not reach Eustress") && failed.contains("Try again"), "{failed}");
        assert!(!failed.contains("You have not published"), "a failed read is not an empty account");
        for loading in [render(Viewer::SignedIn, Load::Loading), render(Viewer::Unknown, Load::Ready(vec![]))] {
            assert!(loading.contains("aria-busy=\"true\""), "{loading}");
            for claim in ["You have not published", "could not load", "Sign in to see", "<dt>"] {
                assert!(!loading.contains(claim), "loading claims {claim}");
            }
        }
        // A search or a status that matches nothing says so, with the way back.
        assert_eq!(shown(&sample(), "zzz", "all", "updated").len(), 0);
        assert!(source().contains("\"No Space matches\"") && source().contains("\"Show all\""));
    }

    #[test]
    fn the_full_page_draws_a_card_per_space_with_its_real_figures() {
        let html = render(Viewer::SignedIn, Load::Ready(sample()));
        assert_eq!(html.matches("class=\"g-card pj-card\"").count(), 3, "{html}");
        for (k, v) in [("Spaces", "3"), ("Visits", "1,208"), ("Published", "1"), ("In review", "1")] {
            assert!(html.contains(&format!("<dt>{k}</dt><dd>{v}</dd>")), "{k} {v}");
        }
        assert!(html.contains("1,204</b> visits") && html.contains("1</b> visit<"));
        assert!(html.contains("Up to 8 players"));
        assert!(html.contains("pj-status published") && html.contains("pj-status review") && html.contains("pj-status archived"));
        // A listed Space's name opens its page; the others are plain names.
        assert!(html.contains("href=\"/simulation/a1\"") && !html.contains("href=\"/simulation/b2\""));
        // The selling chip and the Sell button show only for the Space with an entry.
        assert_eq!(html.matches("sell-chip is-ok").count(), 1);
        assert_eq!(html.matches("href=\"/projects/a1\"").count(), 1);
        // Play only where it can be played; Review only where review has something to say.
        assert_eq!(html.matches("class=\"action-btn primary\"").count(), 1);
        assert!(html.contains("Suggested by review"));
    }

    #[test]
    fn a_card_picture_comes_only_from_https_or_a_path_here() {
        let html = render(Viewer::SignedIn, Load::Ready(sample()));
        assert!(html.contains("src=\"/assets/home/hero-city.webp\"") && !html.contains("evil.example"), "{html}");
        for (url, ok) in [("https://cdn.eustress.dev/a.png", true), ("/assets/a.png", true), ("//evil.example/a.png", false), ("javascript:alert(1)", false), ("http://a.example/a", false)] {
            assert_eq!(card_art(Some(url)).is_some(), ok, "{url}");
        }
        // No person's picture is drawn on this page; every img is a capture or an icon under /assets.
        for chunk in html.split("<img ").skip(1) {
            let tag = &chunk[..chunk.find('>').unwrap()];
            assert!(tag.contains("src=\"/assets/") || tag.contains("src=\"https://"), "{tag}");
        }
    }

    /// What a card's picture area draws for this capture, rendered on the server. art_ok is the signal the img's on:error clears.
    fn art_html(thumbnail: Option<&str>, art_ok: bool) -> String {
        use leptos::tachys::view::RenderHtml;
        let owner = Owner::new();
        owner.with(|| card_picture(thumbnail.map(str::to_string), RwSignal::new(art_ok)).to_html())
    }

    /// The img tag in the page's code that carries this attribute.
    fn art_tag(code: &str, attr: &str) -> String {
        let at = code.find(attr).unwrap_or_else(|| panic!("{attr} is not in the page"));
        let start = code[..at].rfind("<img").expect("an img tag");
        let end = start + code[start..].find("/>").expect("the tag ends");
        code[start..end].to_string()
    }

    #[test]
    fn a_card_picture_that_will_not_load_is_the_cube_a_card_without_one_draws() {
        let url = "https://api.example.dev/a/thumbnail";
        let loading = art_html(Some(url), true);
        assert!(loading.contains("class=\"pj-img\"") && loading.contains(url) && !loading.contains("pj-art-empty"), "{loading}");
        let none = art_html(None, true);
        assert!(none.contains("class=\"pj-art-empty\"><img src=\"/assets/icons/cube.svg\""), "{none}");
        // art_ok is false once the img's on:error has run: the card then draws exactly what it draws for a Space with no capture.
        let failed = art_html(Some(url), false);
        assert_eq!(failed, none, "a capture that failed shows the empty state, not the browser's broken-image icon");
        assert!(!failed.contains("pj-img") && !failed.contains(url));
    }

    #[test]
    fn the_cards_picture_has_an_error_handler_that_clears_its_own_signal() {
        let code = page_source();
        let tag = art_tag(code, "class=\"pj-img\"");
        assert!(tag.contains("on:error=move |_| art_ok.set(false)"), "{tag}");
        assert!(code.contains("move || match thumbnail.clone().filter(|_| art_ok.get())"), "the picture is drawn only while art_ok");
        // One signal for each card: a signal shared by the cards would blank every card at the first failure.
        assert!(code.contains("{card_picture(thumbnail, RwSignal::new(true))}"));
        assert_eq!(code.matches("class=\"pj-img\"").count(), 1, "one picture tag on the card");
    }

    #[test]
    fn a_space_without_a_picture_shows_a_neutral_cube_never_a_gamepad() {
        // Eustress leads as a simulation and data platform: a card with no capture draws the cube, not a game controller.
        let html = render(Viewer::SignedIn, Load::Ready(sample()));
        assert_eq!(html.matches("class=\"pj-art-empty\"><img src=\"/assets/icons/cube.svg\"").count(), 2, "{html}");
        assert!(!html.contains(concat!("game", "pad")), "no gamepad on the page");
        assert!(!page_source().contains(concat!("game", "pad")), "projects.rs names no gamepad icon");
    }

    #[test]
    fn the_tag_chips_link_to_the_gallerys_tag_filter() {
        let html = render(Viewer::SignedIn, Load::Ready(sample()));
        for tag in ["physics", "co-op", "city"] {
            assert!(html.contains(&format!("href=\"/gallery?tag={tag}\"")), "{tag}");
        }
        assert!(!html.contains("category="), "no chip links to a category");
        let chips = html.matches("class=\"g-chip g-tag\"").count();
        assert_eq!(chips, 3);
        assert_eq!(html.matches("href=\"/gallery?tag=").count(), chips, "every tag chip is a Gallery tag filter");
        assert!(html.contains("No tags yet"), "a Space with no tags says so");
        // The search finds a Space by its tag.
        assert_eq!(shown(&sample(), "#co", "all", "updated").iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["a1"]);
    }

    #[test]
    fn the_order_and_the_status_chips_do_what_they_say() {
        let mut list = sample();
        list[2].updated_at = "2026-10-14T09:00:00Z".to_string();
        let ids = |v: Vec<Place>| v.into_iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(shown(&list, "", "all", "updated")), ["c3", "a1", "b2"]);
        assert_eq!(ids(shown(&list, "", "all", "visits")), ["a1", "b2", "c3"]);
        assert_eq!(ids(shown(&list, "", "all", "name")), ["b2", "a1", "c3"]);
        assert_eq!(ids(shown(&list, "", "review", "updated")), ["b2"]);
        assert_eq!(status_chips(&list).iter().map(|c| (c.0, c.2)).collect::<Vec<_>>(), [("all", 3), ("published", 1), ("draft", 0), ("review", 1), ("archived", 1)]);
        let mut private = list.clone();
        private[0].status = SpaceStatus::Private;
        assert!(status_chips(&private).iter().any(|c| c.0 == "private" && c.2 == 1), "a rarer status gets a chip when a Space has it");
        assert_eq!(edited_words("2026-10-12T09:00:00Z", 1_791_000_000_000.0 + 86_400_000.0 * 11.0).as_deref(), Some("Updated yesterday"));
        assert_eq!(edited_words("", 0.0), None);
        assert_eq!(room_words(1), "Solo");
        // The order chosen is the one drawn.
        assert!(source().contains("shown(l, &search_query.get(), &filter_status.get(), &sort_by.get())"));
        assert!(source().contains("key=card_key"), "a card whose status changes is drawn again");
    }

    #[test]
    fn the_money_and_identity_calls_are_the_ones_the_page_made_before() {
        let page = page_source();
        for needle in [
            "match get_commerce_account(&api_url, CommerceMode::Test).await {",
            "match client.get::<ProjectsResponse>(\"/api/projects\").await {",
            "let route = if archive { \"archive\" } else { \"unarchive\" };",
            "client.post::<serde_json::Value, _>(&format!(\"/api/simulations/{}/{route}\", place.id), &serde_json::json!({})).await;",
            "<TagEditor place_id=id.clone() place_name=tags_name tags=tags_now tags_source=tags_source locked=tags_locked on_saved=after_tags />",
            "seller.get().map(|sim| view! { <SellChip sim=sim /> })",
            "seller.get().map(|sim| view! { <SellButton sim_id=sim.id /> })",
        ] {
            assert_eq!(page.matches(needle).count(), 1, "projects.rs: {needle}");
        }
        assert!(!page.contains("/api/keys") && !page.contains("/api/commerce/products"), "the page itself makes no other money or key call");
        // The API keys section the AI page draws: its routes, its payload and its passkey rule.
        let keys = &source()[source().find("pub fn ApiKeysSection").unwrap()..];
        for needle in [
            "client.get::<ApiKeysResponse>(\"/api/keys\").await",
            "\"name\": name,",
            "\"key_type\": key_type,",
            "\"mode\": mode",
            "client.post::<CreateKeyResponse, _>(\"/api/keys\", &body).await",
            "client.delete::<serde_json::Value>(&format!(\"/api/keys/{key_id}\")).await",
            "disabled=move || passkey_session()",
            "crate::pages::passkeys::key_change_blocked(a.user())",
            "crate::pages::passkeys::identity_file_required_words(&e)",
        ] {
            assert!(keys.contains(needle), "ApiKeysSection: {needle}");
        }
        // The signed-out read is the same read; nothing is asked of the Worker to decide who is signed in.
        assert_eq!(page.matches("\"/api/projects\"").count(), 1);
    }

    #[test]
    fn the_copy_has_no_dashes_and_names_the_product_plainly() {
        let pages = [
            render(Viewer::SignedIn, Load::Ready(sample())),
            render(Viewer::SignedIn, Load::Ready(vec![])),
            render(Viewer::SignedOut, Load::Ready(vec![])),
            render(Viewer::SignedIn, Load::Failed("x".to_string())),
            render(Viewer::SignedIn, Load::Loading),
        ];
        for dash in ['\u{2014}', '\u{2013}'] {
            assert!(!page_source().contains(dash), "projects.rs");
            assert!(!CSS.contains(dash) && !SELL_CSS.contains(dash) && !TAGS_CSS.contains(dash), "the page's CSS");
            for html in &pages {
                assert!(!html.contains(dash), "the rendered page");
            }
        }
        for text in pages.iter().map(|h| h.to_lowercase()).chain(std::iter::once(page_source().to_lowercase())) {
            assert!(!text.contains("game engine"), "Eustress is not called a game engine");
            assert!(!text.contains("experience"), "the product noun is Simulation or Space");
        }
    }

    #[test]
    fn projects_css_is_linked_once_and_before_topbar_css() {
        let link = "<link data-trunk rel=\"css\" href=\"style/projects.css\">";
        assert_eq!(INDEX.matches(link).count(), 1, "projects.css is linked once");
        let at = INDEX.find(link).unwrap();
        let topbar = INDEX.find("<link data-trunk rel=\"css\" href=\"style/topbar.css\">").expect("topbar.css is linked");
        assert!(at < topbar, "projects.css comes before topbar.css, which stays last");
        assert!(CSS.contains(".pj-hero") && CSS.contains(".pj-card") && CSS.contains("@media (max-width: 860px)"));
        // The selling chip and the card's buttons follow the card the page draws now.
        assert!(SELL_CSS.contains(".pj-card .sell-chip") && !SELL_CSS.contains(concat!(".place", "-card")));
    }

    #[test]
    fn the_old_pages_rules_are_gone_from_main_css() {
        for gone in [concat!(".page-projects-", "industrial"), concat!(".projects-", "glow"), concat!(".projects-", "title"), concat!(".ai-protocol-", "card"), concat!(".place", "-card"), concat!(".ai-bench-", "row")] {
            assert!(!MAIN_CSS.contains(&format!("{gone} ")), "{gone} is still in main.css");
        }
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn the_projects_address_shows_this_page() {
        use hydration_context::{SharedContext, SsrSharedContext};
        use leptos::tachys::view::RenderHtml;
        use leptos_router::location::RequestUrl;
        let _ = leptos::task::Executor::init_futures_executor();
        let shared: std::sync::Arc<dyn SharedContext + Send + Sync> = std::sync::Arc::new(SsrSharedContext::new());
        let owner = Owner::new_root(Some(shared));
        let html = owner.with(|| {
            provide_context(RequestUrl::new("/projects"));
            crate::app::App().to_html()
        });
        owner.cleanup();
        assert!(html.contains("pj-page") && html.contains(">Your projects</h1>"), "{html}");
        assert!(html.contains("aria-busy=\"true\""), "before the sign-in is known, the page shows the loading shape");
    }
}
