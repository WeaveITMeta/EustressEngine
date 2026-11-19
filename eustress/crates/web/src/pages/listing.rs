// =============================================================================
// Eustress Web - A listing's page (/simulation/:id)
// =============================================================================
// One Space: its pictures, who made it, what it is, and the one Play area its state allows. Everything shown is what the Worker
// said about this listing:
//
//   GET /api/simulations/{id}            the listing (and, for its author, the review)
//   GET /api/simulations/{id}/live       is it hosted now
//   GET /api/simulations/{id}/previews   the captures Studio took at publish
//   GET /api/simulations/{id}/page       the themed page and the README (api/sim_page.rs)
//
// Under the short description sit four tabs (docs/design/SIM_PAGE_README_AND_TABS.md, section 4): Servers (live status), Shop (the
// Space's passes), Links (the approved page links) and About (the README, components/readme.rs). Links and About are shown only
// when there is something in them. Details sits in the right column, under the download and Studio buttons.
//
// Every Play button here opens the Play chooser in place (components/play_chooser.rs); the Play area itself is the same panel the
// chooser draws, so the page and the dialog never disagree. A Space that is not in the Gallery is its author's to see, and everyone
// else gets "not found". `/play/<id>` lands here with `?play=1`, which opens the chooser once the listing has loaded.
//
// Table of Contents:
// 1. Wording (pure, tested)
// 2. The page
// 3. The pictures
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::catalog::{self, art_of, card_tags, is_multiplayer, published_words, rating_words, size_words, suggested_tags_label, ArtKind, Listing};
use crate::api::ApiClient;
use crate::api::live::{latency_label, LiveServer};
use crate::api::sim_page::{self as sim_api, SimPage};
use crate::pages::listing_edit::ListingEditDialog;
use crate::pages::sim_page::{banner_view, gallery_view, links_view, logo_view, theme_of};
use crate::components::play_chooser::{now_ms, status_of, watch, Facts, Live, Load, PlayChooserSlot, PlayStatePanel, Status, Viewer};
use crate::components::{CentralNav, Footer, PassStore};
use crate::components::readme::{has_readme, readme_view};
use crate::components::adult_content::{Age21ListingNotice, AdultListingNotice};
use crate::pages::review_panel::status_label;
use crate::state::{AppState, AuthState};
use crate::tags::gallery_href;

// -----------------------------------------------------------------------------
// 1. Wording
// -----------------------------------------------------------------------------

/// One server row: seats, region, the API round trip, and the hardware class.
fn server_caption(server: &LiveServer) -> String {
    let hardware = server.hardware.as_ref().map(|h| h.tier.as_str()).filter(|t| !t.is_empty()).unwrap_or("unknown");
    let region = server.region.as_deref().filter(|r| !r.is_empty()).unwrap_or("region not set");
    let seats = if server.full {
        format!("{} of {} players, full", server.players, server.max_players)
    } else {
        format!("{} of {} players, {} free", server.players, server.max_players, server.free)
    };
    format!("{seats} · {region} · {} · {hardware}", latency_label(server.latency_ms.filter(|ms| *ms >= 1)))
}

/// The pictures of a listing in the order the page shows them: the capture Studio took at publish, then the review captures, then
/// the cover when there is nothing else. An address that is not https or a path is never used, and one picture is never shown twice.
pub fn frames_of(listing: &Listing, previews: &[String]) -> Vec<String> {
    fn push(out: &mut Vec<String>, u: &str) {
        if (u.starts_with("https://") || u.starts_with('/')) && !out.iter().any(|seen| seen == u) {
            out.push(u.to_string());
        }
    }
    let mut out: Vec<String> = Vec::new();
    if let Some(t) = &listing.thumbnail_url {
        push(&mut out, t);
    }
    for p in previews {
        push(&mut out, p);
    }
    if out.is_empty() {
        if let Some((ArtKind::Cover, url)) = art_of(listing) {
            push(&mut out, &url);
        }
    }
    out
}

/// The pictures still worth showing: the pictures less every address the browser failed to load. The Worker answers 404 for a
/// capture it will not serve (animated, transparent, several images, not an image, too large), so a picture that fails is left
/// out of the frame and of the strip. When none is left the page shows what it shows for a Space with no picture at all.
pub fn without_broken(frames: &[String], broken: &[String]) -> Vec<String> {
    frames.iter().filter(|u| !broken.contains(u)).cloned().collect()
}

/// "Capture 2 / 4": which picture is up. The reader is one-based.
pub fn capture_words(index: usize, count: usize) -> String {
    format!("Capture {} / {}", index.min(count.saturating_sub(1)) + 1, count)
}

/// How long ago, for the Details list: "3 days ago", "today", or the month and year.
pub fn ago_words(iso: &str, now_ms: f64) -> Option<String> {
    published_words(iso, now_ms).map(|w| {
        let w = w.trim_start_matches("published ").to_string();
        let mut chars = w.chars();
        chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or(w)
    })
}

/// The rows of the Details list: only what the Worker said, in a fixed order.
pub fn detail_rows(listing: &Listing, now_ms: f64) -> Vec<(&'static str, String)> {
    let mut rows = Vec::new();
    rows.push((
        "Players",
        if is_multiplayer(listing) { format!("Up to {} (set when it was published)", listing.max_players) } else { "One (solo)".to_string() },
    ));
    if let Some(bytes) = listing.scene_size_bytes {
        rows.push(("Size", size_words(bytes)));
    }
    if let Some(p) = listing.published_at.as_deref().and_then(|iso| ago_words(iso, now_ms)) {
        rows.push(("Published", p));
    }
    if let Some(u) = listing.updated_at.as_deref().and_then(|iso| ago_words(iso, now_ms)) {
        rows.push(("Updated", u));
    }
    if let Some(r) = rating_words(listing.rating.as_deref()) {
        rows.push(("Rating", r.to_string()));
    }
    rows
}

/// The crumb between "Gallery" and the Space: its first tag, as the link that filters the Gallery to it.
pub fn crumb_tag(listing: &Listing) -> Option<String> {
    card_tags(listing).into_iter().next()
}

/// The tabs under the description (docs/design/SIM_PAGE_README_AND_TABS.md, section 4), in the order the bar shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Servers,
    Shop,
    Links,
    About,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Servers, Tab::Shop, Tab::Links, Tab::About];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Servers => "Servers",
            Tab::Shop => "Shop",
            Tab::Links => "Links",
            Tab::About => "About",
        }
    }

    /// The word in the tab's and its panel's element ids.
    pub fn key(self) -> &'static str {
        match self {
            Tab::Servers => "servers",
            Tab::Shop => "shop",
            Tab::Links => "links",
            Tab::About => "about",
        }
    }

    fn index(self) -> usize {
        match self {
            Tab::Servers => 0,
            Tab::Shop => 1,
            Tab::Links => 2,
            Tab::About => 3,
        }
    }
}

/// Whether the page has an approved link to show (the Links tab).
pub fn page_has_links(page: &Option<SimPage>) -> bool {
    page.as_ref().is_some_and(|p| !sim_api::visible_links(&p.links).is_empty())
}

/// Whether the page has a README that draws anything (the About tab).
pub fn page_has_readme(page: &Option<SimPage>) -> bool {
    page.as_ref().and_then(|p| p.readme.as_ref()).is_some_and(has_readme)
}

/// The tabs shown: Servers and Shop always, Links and About only when they have something.
pub fn visible_tabs(links: bool, about: bool) -> Vec<Tab> {
    Tab::ALL.into_iter().filter(|t| match t {
        Tab::Links => links,
        Tab::About => about,
        _ => true,
    }).collect()
}

/// The tab that is up: the one the visitor chose, while it is shown; before they choose, About when there is one, else Servers.
/// The README arrives after the listing, so a visitor who has not chosen moves to About when it comes.
pub fn shown_tab(chosen: Option<Tab>, visible: &[Tab]) -> Tab {
    match chosen {
        Some(t) if visible.contains(&t) => t,
        _ if visible.contains(&Tab::About) => Tab::About,
        _ => Tab::Servers,
    }
}

/// The tab a key moves to from `current`: left and right step through the shown tabs and wrap, Home and End go to the ends.
/// Any other key moves nothing.
pub fn key_tab(current: Tab, visible: &[Tab], key: &str) -> Option<Tab> {
    let n = visible.len();
    if n == 0 {
        return None;
    }
    let at = visible.iter().position(|t| *t == current).unwrap_or(0);
    match key {
        "ArrowRight" => Some(visible[(at + 1) % n]),
        "ArrowLeft" => Some(visible[(at + n - 1) % n]),
        "Home" => Some(visible[0]),
        "End" => Some(visible[n - 1]),
        _ => None,
    }
}

// -----------------------------------------------------------------------------
// 2. The page
// -----------------------------------------------------------------------------

#[component]
pub fn ListingPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let chooser = expect_context::<PlayChooserSlot>();
    let params = leptos_router::hooks::use_params_map();
    let query = leptos_router::hooks::use_query_map();
    let id = move || params.read().get("id").unwrap_or_default();

    let load = RwSignal::new(Load::Loading);
    let live = RwSignal::new(Live::Unknown);
    let runs = StoredValue::new(0u64);
    let opened = StoredValue::new(false);

    // Start over when the route's id changes. An Effect, so the polling runs only in the browser.
    Effect::new(move |_| {
        let sim = id();
        let run = runs.get_value() + 1;
        runs.set_value(run);
        opened.set_value(false);
        load.set(Load::Loading);
        live.set(Live::Unknown);
        spawn_local(watch(api_url.get_value(), sim, load, live, runs, run));
    });

    // `?play=1` (what /play/<id> sends people to): once the listing is there, open the chooser once and take the mark off the address.
    Effect::new(move |_| {
        if let Load::Ready(listing) = load.get() {
            if !opened.get_value() && query.read().get("play").as_deref() == Some("1") {
                opened.set_value(true);
                crate::utils::replace_path(&format!("/simulation/{}", urlencoding::encode(&listing.id)));
                chooser.open(&listing.id);
            }
        }
    });

    view! {
        <div class="page g-page">
            <CentralNav active="gallery".to_string() />
            <div class="g-backdrop" aria-hidden="true"></div>
            <main class="g-main g-listing">
                {move || match load.get() {
                    Load::Loading => view! { <ListingSkeleton /> }.into_any(),
                    Load::Missing => view! { <ListingMissing /> }.into_any(),
                    Load::AdultOff { signed_in } => view! { <AdultListingNotice signed_in=signed_in /> }.into_any(),
                    Load::Age21Required(message) => view! { <Age21ListingNotice message=message /> }.into_any(),
                    Load::Failed(why) => view! { <ListingFailed why=why /> }.into_any(),
                    Load::Ready(listing) => view! { <ListingBody listing=listing live=live /> }.into_any(),
                }}
            </main>
            <Footer />
        </div>
    }
}

/// Note that a picture failed to load. Once: the frame and the strip draw the same address.
fn mark_broken(broken: RwSignal<Vec<String>>, url: &str) {
    if !broken.with_untracked(|list| list.iter().any(|u| u == url)) {
        broken.update(|list| list.push(url.to_string()));
    }
}

/// The frame: every picture stacked, the one at shot up. A picture that fails goes into broken and drops out of the list; with
/// none left the frame is the quiet one a Space without a picture gets.
fn shot_frame(list: Vec<String>, shot: RwSignal<usize>, broken: RwSignal<Vec<String>>) -> AnyView {
    if list.is_empty() {
        return view! { <div class="g-frame-empty" aria-hidden="true"></div> }.into_any();
    }
    let count = list.len();
    view! {
        {list.into_iter().enumerate().map(|(i, src)| {
            let url = src.clone();
            view! {
                <img class="art-img" class:on=move || shot.get().min(count - 1) == i src=src alt="" decoding="async" on:error=move |_| mark_broken(broken, &url) />
            }
        }).collect_view()}
        <div class="g-readout" aria-hidden="true"><span>{move || capture_words(shot.get(), count)}</span><span>"At publish"</span></div>
    }.into_any()
}

/// The strip of captures, drawn when two or more pictures are left. A thumbnail that fails drops out with its picture.
fn shot_caps(list: Vec<String>, shot: RwSignal<usize>, broken: RwSignal<Vec<String>>) -> impl IntoView {
    (list.len() > 1).then(|| view! {
        <div class="g-caps" role="group" aria-label="Captures">
            {list.into_iter().enumerate().map(|(i, src)| {
                let url = src.clone();
                view! {
                    <button type="button" class="g-cap" class:on=move || shot.get() == i aria-label=format!("Show capture {}", i + 1) aria-pressed=move || (shot.get() == i).to_string() on:click=move |_| shot.set(i)>
                        <img src=src alt="" loading="lazy" decoding="async" on:error=move |_| mark_broken(broken, &url) />
                    </button>
                }
            }).collect_view()}
        </div>
    })
}

#[component]
fn ListingBody(listing: Listing, live: RwSignal<Live>) -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    let api_url = StoredValue::new(app_state.api_url.clone());
    let chooser = expect_context::<PlayChooserSlot>();
    let viewer = Viewer::detect();
    let now = now_ms();

    let id = listing.id.clone();
    let name = if listing.name.trim().is_empty() { "Untitled Space".to_string() } else { listing.name.clone() };
    let status = status_of(&listing);
    let moderation = listing.moderation.clone().unwrap_or_default();
    let person_review = moderation.person_review;
    let author = listing.author_name.clone().filter(|n| !n.trim().is_empty());
    let author_href = author.as_ref().map(|a| format!("/profile/{}", urlencoding::encode(a)));
    let initial = author.as_ref().and_then(|a| a.chars().next()).map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let author_id = listing.author_id.clone();
    let is_author = move || match auth.get() {
        AuthState::Authenticated(user) => user.id.to_string().eq_ignore_ascii_case(&author_id),
        _ => false,
    };

    // The review captures, asked for once; the pictures follow.
    let previews = RwSignal::new(Vec::<String>::new());
    {
        let id = id.clone();
        Effect::new(move |_| {
            let id = id.clone();
            spawn_local(async move {
                let client = ApiClient::new(&api_url.get_value());
                previews.set(catalog::fetch_previews(&client, &id).await.into_iter().map(|p| p.url).collect());
            });
        });
    }
    // The themed page the creator set and the Worker approved (docs/design/SIM_PAGE_THEME_AND_LINKS.md). Nothing, for any failure:
    // the page is then exactly what it is without one.
    let page = RwSignal::new(None::<SimPage>);
    {
        let id = id.clone();
        Effect::new(move |_| {
            let id = id.clone();
            spawn_local(async move {
                let client = ApiClient::new(&api_url.get_value());
                page.set(sim_api::fetch_page(&client, &id).await);
            });
        });
    }
    let frames = {
        let listing = listing.clone();
        Memo::new(move |_| frames_of(&listing, &previews.get()))
    };
    let shot = RwSignal::new(0usize);
    // The pictures the browser could not load; the frame and the strip draw the rest.
    let broken = RwSignal::new(Vec::<String>::new());
    let shown = Memo::new(move |_| frames.with(|all| broken.with(|bad| without_broken(all, bad))));

    let rating = rating_words(listing.rating.as_deref());
    let tags = card_tags(&listing);
    // "Suggested by review" is for the author alone, and only when the review chose the tags.
    let tags_label = {
        let listing = listing.clone();
        let is_author = is_author.clone();
        move || suggested_tags_label(&listing, is_author())
    };
    let crumb = crumb_tag(&listing);
    let details = detail_rows(&listing, now);
    let description = listing.description.trim().to_string();
    let visits = listing.play_count;
    let favorites = listing.favorite_count;
    let multiplayer = is_multiplayer(&listing);
    let review_href = format!("/projects?review={id}");
    let headline = moderation.headline.clone();
    let reasons = moderation.reasons.clone();
    let suggested = moderation.suggested_edit.clone();
    let note = moderation.note.clone();
    let play_id = id.clone();
    let join_id = id.clone();
    // Share Source is on: anyone may open a copy in Studio (a fork into a new Universe of their own).
    let studio_copy = listing.studio_copy_link();
    let is_open = studio_copy.is_some();
    // The author edits the title, description, visibility and tags here (pages/listing_edit.rs).
    let editing = RwSignal::new(false);
    let edit_listing = StoredValue::new(listing.clone());
    let can_edit = is_author.clone();

    let playing = move || match live.get() {
        Live::Hosted { players, .. } => players,
        _ => 0,
    };

    // The tabs under the description. Which are shown follows the page; which is up follows the visitor's choice, and before they
    // choose it is About when the README is there (it arrives after the listing) and Servers otherwise.
    let shop_id = id.clone();
    let chosen = RwSignal::new(None::<Tab>);
    let tabs = Memo::new(move |_| page.with(|p| visible_tabs(page_has_links(p), page_has_readme(p))));
    let current = Memo::new(move |_| tabs.with(|v| shown_tab(chosen.get(), v)));
    let tab_refs: [NodeRef<leptos::html::Button>; 4] = [NodeRef::new(), NodeRef::new(), NodeRef::new(), NodeRef::new()];

    view! {
        <div
            class=move || sim_api::root_class(theme_of(&page.get()).as_ref())
            style=move || sim_api::root_style(theme_of(&page.get()).as_ref())
        >
        {move || banner_view(&page.get(), &api_url.get_value())}
        <div class="g-wrap">
            <nav class="g-crumbs" aria-label="Breadcrumb">
                <a href="/gallery">"Gallery"</a>
                {crumb.map(|t| {
                    let href = gallery_href(std::slice::from_ref(&t));
                    view! { <span aria-hidden="true">"/"</span><a href=href>{t}</a> }
                })}
                <span aria-hidden="true">"/"</span>
                <span class="g-crumb-now" aria-current="page">{name.clone()}</span>
            </nav>

            <div class="g-lhead">
                <div class="g-lmedia">
                    <div class="g-frame g-rise">
                        <span class="g-bracket tl"></span><span class="g-bracket tr"></span><span class="g-bracket bl"></span><span class="g-bracket br"></span>
                        {move || shot_frame(shown.get(), shot, broken)}
                    </div>
                    {move || shot_caps(shown.get(), shot, broken)}
                </div>

                <aside class="g-lside">
                    <div class="g-chips">
                        {tags.into_iter().map(|t| {
                            let href = gallery_href(std::slice::from_ref(&t));
                            view! { <a class="g-chip g-tag" href=href>{t}</a> }
                        }).collect_view()}
                        {move || tags_label().map(|words| view! { <span class="g-chip">{words}</span> })}
                        {rating.map(|r| view! { <span class="g-chip">{r}</span> })}
                        <span class="g-chip">{if multiplayer { "Multiplayer" } else { "Solo" }}</span>
                        {(status != Status::Listed).then(|| {
                            let word = status_label(match status {
                                Status::Private => "approved_private",
                                Status::InReview => "in_review",
                                Status::ChangesRequested => "changes_requested",
                                Status::NotListed => "not_listed",
                                Status::AppealInReview => "appeal_in_review",
                                Status::Unreviewed => "unreviewed",
                                Status::Listed => "listed",
                            });
                            view! { <span class="g-chip warn">{word}</span> }
                        })}
                        {move || match live.get() {
                            Live::Hosted { players, max_players, .. } if matches!(status, Status::Listed | Status::Private) => {
                                view! { <span class="g-chip live"><i class="g-dot"></i>{format!("Live {players}/{max_players}")}</span> }.into_any()
                            }
                            Live::Offline if matches!(status, Status::Listed | Status::Private) => view! { <span class="g-chip"><i class="g-dot off"></i>"Offline"</span> }.into_any(),
                            _ => ().into_any(),
                        }}
                    </div>
                    <h1 class="g-ltitle">{move || logo_view(&page.get(), &api_url.get_value())}{name.clone()}</h1>
                    {author.zip(author_href).map(|(a, href)| view! {
                        <a class="g-maker" href=href><span class="g-av" aria-hidden="true">{initial}</span>{format!("@{a}")}</a>
                    })}
                    <dl class="g-stats">
                        <div><dt>"Playing"</dt><dd>{move || playing()}</dd></div>
                        <div><dt>"Visits"</dt><dd>{visits}</dd></div>
                        <div><dt>"Favorites"</dt><dd>{favorites}</dd></div>
                    </dl>
                    <div class="g-glass g-lpanel">
                        {move || {
                            let facts = Facts { status, author: is_author(), person_review, live: live.get(), viewer };
                            let open = {
                                let play_id = play_id.clone();
                                Callback::new(move |_| chooser.open(&play_id))
                            };
                            view! {
                                <PlayStatePanel
                                    facts=facts
                                    headline=headline.clone()
                                    reasons=reasons.clone()
                                    suggested=suggested.clone()
                                    note=note.clone()
                                    review_href=review_href.clone()
                                    on_play=open
                                />
                            }
                        }}
                    </div>
                    <div class="g-links">
                        {move || can_edit().then(|| view! {
                            <button type="button" class="g-btn g-btn-primary g-edit-listing" on:click=move |_| editing.set(true)>"Edit listing"</button>
                        })}
                        <a class="g-btn" href="/downloads/player">"Download Eustress Player"</a>
                        {studio_copy.map(|link| view! {
                            <a class="g-btn g-btn-primary g-studio-copy" href=link>"Open a copy in Studio"</a>
                        })}
                    </div>
                    {is_open.then(|| view! {
                        <p class="g-hint g-studio-hint">
                            "The author shares this Space's source. Studio saves your copy as a new Universe of your own; publishing it makes a new listing, and this one stays as it is. Needs "
                            <a href="/download">"Eustress Studio"</a>"."
                        </p>
                    })}
                    <section class="g-csec g-ldetails">
                        <h2>"Details"</h2>
                        <dl class="g-kv g-glass">
                            {details.into_iter().map(|(k, v)| view! { <dt>{k}</dt><dd>{v}</dd> }).collect_view()}
                        </dl>
                    </section>
                </aside>

                <div class="g-lcopy">
                    {move || gallery_view(&page.get(), &api_url.get_value())}
                    {(!description.is_empty()).then(|| view! { <p class="g-about lt-desc">{description}</p> })}
                    <div
                        class="lt-tabs"
                        role="tablist"
                        aria-label="About this Space"
                        on:keydown=move |ev| {
                            let next = tabs.with_untracked(|v| key_tab(current.get_untracked(), v, ev.key().as_str()));
                            if let Some(tab) = next {
                                ev.prevent_default();
                                chosen.set(Some(tab));
                                if let Some(button) = tab_refs[tab.index()].get_untracked() {
                                    let _ = button.focus();
                                }
                            }
                        }
                    >
                        {move || tabs.get().into_iter().map(|tab| view! {
                            <button
                                type="button"
                                role="tab"
                                class="lt-tab"
                                id=format!("lt-tab-{}", tab.key())
                                aria-controls=format!("lt-panel-{}", tab.key())
                                aria-selected=move || (current.get() == tab).to_string()
                                tabindex=move || if current.get() == tab { "0" } else { "-1" }
                                node_ref=tab_refs[tab.index()]
                                on:click=move |_| chosen.set(Some(tab))
                            >
                                {tab.label()}
                            </button>
                        }).collect_view()}
                    </div>
                    <div class="lt-panel" role="tabpanel" id="lt-panel-servers" aria-labelledby="lt-tab-servers" tabindex="0" hidden=move || current.get() != Tab::Servers>
                    {move || {
                        let listed = matches!(status, Status::Listed | Status::Private);
                        match live.get() {
                            Live::Hosted { players, max_players, servers, full, .. } if listed => {
                                let join_id = join_id.clone();
                                let pct = if max_players == 0 { 0 } else { (players.min(max_players) * 100 / max_players).min(100) };
                                let rows = servers.clone();
                                view! {
                                    <section class="g-csec"><h2>"Live status"</h2>
                                        <div class="g-glass g-session">
                                            <span class="g-dot" aria-hidden="true"></span>
                                            <div><b>"Hosted by the author"</b><small>{format!("{players} of {max_players} players")}</small></div>
                                            <div class="g-meter" aria-hidden="true"><i style=format!("width:{pct}%")></i></div>
                                            {(viewer != Viewer::Phone && !full).then(|| { let join_id = join_id.clone(); view! { <button type="button" class="g-btn g-btn-primary g-btn-sm" on:click=move |_| chooser.open(&join_id)>"Join"</button> } })}
                                        </div>
                                        {rows.into_iter().map(|server| {
                                            let caption = server_caption(&server);
                                            let dot = if server.full { "g-dot off" } else { "g-dot" };
                                            let name = if server.id.is_empty() { "Server".to_string() } else { server.id.clone() };
                                            view! {
                                                <div class="g-glass g-session">
                                                    <span class=dot aria-hidden="true"></span>
                                                    <div><b>{name}</b><small>{caption}</small></div>
                                                </div>
                                            }
                                        }).collect_view()}
                                    </section>
                                }.into_any()
                            }
                            Live::Offline if listed => view! {
                                <section class="g-csec"><h2>"Live status"</h2>
                                    <div class="g-glass g-session">
                                        <span class="g-dot off" aria-hidden="true"></span>
                                        <div><b>"Offline: nobody is hosting right now"</b><small>"Playing from here runs it on your own computer."</small></div>
                                    </div>
                                </section>
                            }.into_any(),
                            _ if !listed => view! { <p class="g-hint lt-empty">"Servers show here once this Space is listed."</p> }.into_any(),
                            _ => view! { <p class="g-hint lt-empty">"No live status to show right now."</p> }.into_any(),
                        }
                    }}
                    </div>
                    <div class="lt-panel lt-shop" role="tabpanel" id="lt-panel-shop" aria-labelledby="lt-tab-shop" tabindex="0" hidden=move || current.get() != Tab::Shop>
                        <PassStore sim_id=shop_id />
                        <p class="g-hint lt-empty lt-shop-empty">"Nothing for sale in this Space yet."</p>
                    </div>
                    {move || page_has_links(&page.get()).then(|| view! {
                        <div class="lt-panel" role="tabpanel" id="lt-panel-links" aria-labelledby="lt-tab-links" tabindex="0" hidden=move || current.get() != Tab::Links>
                            {links_view(&page.get_untracked())}
                        </div>
                    })}
                    {move || page.with(|p| p.as_ref().and_then(|p| p.readme.clone())).filter(has_readme).map(|readme| view! {
                        <div class="lt-panel lt-about" role="tabpanel" id="lt-panel-about" aria-labelledby="lt-tab-about" tabindex="0" hidden=move || current.get() != Tab::About>
                            {readme_view(&readme, &api_url.get_value())}
                        </div>
                    })}
                </div>
            </div>
        </div>
        {move || editing.get().then(|| view! {
            <ListingEditDialog listing=edit_listing.get_value() on_close=Callback::new(move |_| editing.set(false)) />
        })}
        </div>
    }
}

// -----------------------------------------------------------------------------
// 3. Waiting, missing, failing
// -----------------------------------------------------------------------------

#[component]
fn ListingSkeleton() -> impl IntoView {
    view! {
        <div class="g-wrap g-skeleton" role="status" aria-busy="true" aria-label="Loading this Space">
            <div class="g-lhead">
                <div class="g-lmedia"><div class="g-skel g-skel-frame"></div></div>
                <div class="g-lside"><span class="g-skel w30"></span><span class="g-skel w80 tall"></span><span class="g-skel w40"></span><div class="g-skel g-skel-panel"></div></div>
            </div>
        </div>
    }
}

#[component]
fn ListingMissing() -> impl IntoView {
    let auth = expect_context::<AppState>().auth;
    view! {
        <div class="g-wrap">
            <div class="g-glass g-none" role="status">
                <h1 class="g-none-title">"We can't find this Space"</h1>
                <p>"It may have been removed, or it is not public yet. If it is yours, sign in to see it."</p>
                <div class="g-actions">
                    {move || (!matches!(auth.get(), AuthState::Authenticated(_))).then(|| view! { <a href="/login" class="g-btn g-btn-primary">"Sign in"</a> })}
                    <a href="/gallery" class="g-btn">"Go to the Gallery"</a>
                </div>
            </div>
        </div>
    }
}

#[component]
fn ListingFailed(why: String) -> impl IntoView {
    view! {
        <div class="g-wrap">
            <div class="g-glass g-none" role="alert">
                <h1 class="g-none-title">"This Space could not load"</h1>
                <p>{why}</p>
                <p class="g-hint">"The page tries again by itself."</p>
                <div class="g-actions"><a href="/gallery" class="g-btn">"Go to the Gallery"</a></div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_791_000_000_000.0;
    const DAY: f64 = 86_400_000.0;

    fn iso(days_ago: f64) -> String {
        chrono::DateTime::from_timestamp_millis((NOW - days_ago * DAY) as i64).unwrap().to_rfc3339()
    }

    fn listing() -> Listing {
        Listing { id: "a".to_string(), name: "A".to_string(), ..Default::default() }
    }

    #[test]
    fn the_capture_comes_first_then_the_review_captures_without_repeats_and_never_an_unsafe_address() {
        let mut l = listing();
        l.thumbnail_url = Some("https://api.example.dev/t".to_string());
        let previews = vec![
            "https://api.example.dev/p/0".to_string(),
            "https://api.example.dev/t".to_string(),
            "javascript:alert(1)".to_string(),
            "/api/simulations/a/preview/1".to_string(),
        ];
        assert_eq!(frames_of(&l, &previews), ["https://api.example.dev/t", "https://api.example.dev/p/0", "/api/simulations/a/preview/1"]);
    }

    #[test]
    fn a_cover_is_the_picture_only_when_there_is_nothing_else() {
        let mut l = listing();
        l.cover_url = Some("https://api.example.dev/c".to_string());
        assert_eq!(frames_of(&l, &[]), ["https://api.example.dev/c"]);
        l.thumbnail_url = Some("https://api.example.dev/t".to_string());
        assert_eq!(frames_of(&l, &[]), ["https://api.example.dev/t"]);
        assert!(frames_of(&listing(), &[]).is_empty(), "with no picture the frame stays quiet");
    }

    /// The frame and the strip for these pictures, with these addresses already failed, rendered on the server.
    fn shots_html(all: &[&str], failed: &[&str]) -> (String, String) {
        use leptos::tachys::view::RenderHtml;
        let owner = Owner::new();
        owner.with(|| {
            let all: Vec<String> = all.iter().map(|u| u.to_string()).collect();
            let broken = RwSignal::new(failed.iter().map(|u| u.to_string()).collect::<Vec<_>>());
            let list = without_broken(&all, &broken.get_untracked());
            (shot_frame(list.clone(), RwSignal::new(0), broken).to_html(), shot_caps(list, RwSignal::new(0), broken).to_html())
        })
    }

    #[test]
    fn a_picture_the_browser_cannot_load_is_left_out_and_with_none_left_the_frame_is_the_quiet_one() {
        let all = ["https://a.example/t", "https://a.example/p0", "https://a.example/p1"].map(String::from);
        assert_eq!(without_broken(&all, &[]), all);
        assert_eq!(without_broken(&all, &["https://a.example/p0".to_string()]), ["https://a.example/t", "https://a.example/p1"]);
        assert_eq!(without_broken(&all, &["https://elsewhere.example/x".to_string()]), all, "an address that is not shown changes nothing");
        assert!(without_broken(&all, &all).is_empty());
        let urls = ["https://a.example/t", "https://a.example/p0", "https://a.example/p1"];
        // Nothing failed: three pictures in the frame, three thumbnails in the strip.
        let (frame, caps) = shots_html(&urls, &[]);
        assert_eq!(frame.matches("<img").count(), 3, "{frame}");
        assert!(frame.contains("Capture 1 / 3") && !frame.contains("g-frame-empty"), "{frame}");
        assert!(caps.contains("g-caps") && caps.matches("<img").count() == 3, "{caps}");
        // One failed: it is gone from both, and the reader counts what is left.
        let (frame, caps) = shots_html(&urls, &["https://a.example/p0"]);
        assert!(frame.matches("<img").count() == 2 && !frame.contains("/p0") && frame.contains("Capture 1 / 2"), "{frame}");
        assert!(caps.matches("<img").count() == 2 && !caps.contains("/p0"), "{caps}");
        // One left: the frame keeps it, and a strip of one is not drawn.
        let (frame, caps) = shots_html(&urls, &["https://a.example/p0", "https://a.example/p1"]);
        assert!(frame.matches("<img").count() == 1 && frame.contains("/t"), "{frame}");
        assert!(!caps.contains("g-caps") && !caps.contains("<img"), "{caps}");
        // All failed: what the page shows with no pictures at all, and no strip.
        let (frame, caps) = shots_html(&urls, &urls);
        assert!(frame.contains("g-frame-empty") && !frame.contains("<img") && !frame.contains("g-readout"), "{frame}");
        assert!(!caps.contains("g-caps") && !caps.contains("<img"), "{caps}");
        let (none_frame, _) = shots_html(&[], &[]);
        assert_eq!(frame, none_frame, "every picture failing looks like a Space that has none");
    }

    #[test]
    fn each_picture_on_the_page_has_an_error_handler_and_the_page_draws_the_pictures_that_remain() {
        let page = include_str!("listing.rs").split(concat!("#[cfg", "(test)]")).next().unwrap();
        let tag_of = |attr: &str| {
            let at = page.find(attr).unwrap_or_else(|| panic!("{attr} is not in the page"));
            let start = page[..at].rfind("<img").expect("an img tag");
            page[start..start + page[start..].find("/>").expect("the tag ends")].to_string()
        };
        let frame_img = tag_of("class=\"art-img\"");
        let strip_img = tag_of("src=src alt=\"\" loading=\"lazy\"");
        for tag in [frame_img, strip_img] {
            assert!(tag.contains("on:error=move |_| mark_broken(broken, &url)"), "{tag}");
        }
        assert_eq!(page.matches("<img").count(), 2, "the frame's picture and the strip's thumbnail are the page's only imgs");
        assert!(page.contains("{move || shot_frame(shown.get(), shot, broken)}") && page.contains("{move || shot_caps(shown.get(), shot, broken)}"));
        assert!(page.contains("frames.with(|all| broken.with(|bad| without_broken(all, bad)))"), "the pictures that remain");
        assert!(!page.contains("frames.get()"), "the page never draws the pictures that failed");
    }

    #[test]
    fn the_capture_reader_is_one_based_and_cannot_run_past_the_end() {
        assert_eq!(capture_words(0, 4), "Capture 1 / 4");
        assert_eq!(capture_words(3, 4), "Capture 4 / 4");
        assert_eq!(capture_words(9, 4), "Capture 4 / 4");
        assert_eq!(capture_words(0, 0), "Capture 1 / 0");
    }

    #[test]
    fn dates_read_as_a_sentence_start_and_a_missing_one_is_left_out() {
        assert_eq!(ago_words(&iso(0.0), NOW), Some("Today".to_string()));
        assert_eq!(ago_words(&iso(1.0), NOW), Some("Yesterday".to_string()));
        assert_eq!(ago_words(&iso(3.0), NOW), Some("3 days ago".to_string()));
        assert_eq!(ago_words("not a date", NOW), None);
    }

    #[test]
    fn the_details_list_has_only_what_the_worker_said() {
        let mut l = listing();
        l.max_players = 10;
        l.scene_size_bytes = Some(2_562_226);
        l.published_at = Some(iso(1.0));
        l.rating = Some("teen_13".to_string());
        l.tags = vec!["horror".to_string()];
        let rows = detail_rows(&l, NOW);
        let keys: Vec<&str> = rows.iter().map(|r| r.0).collect();
        assert_eq!(keys, ["Players", "Size", "Published", "Rating"], "no Updated row without a date, no category row, nothing invented");
        assert_eq!(rows[0].1, "Up to 10 (set when it was published)");
        assert_eq!(rows[1].1, "2.4 MB");
        assert_eq!(detail_rows(&listing(), NOW)[0].1, "One (solo)");
    }

    #[test]
    fn the_page_asks_for_the_review_label_as_the_reader_and_never_as_the_author_by_default() {
        let live = include_str!("listing.rs").split("#[cfg(test)]").next().unwrap();
        assert!(live.contains("suggested_tags_label(&listing, is_author())"));
        assert!(!live.contains("suggested_tags_label(&listing, true)"));
    }

    /// The page's own source, without the tests.
    fn page_source() -> &'static str {
        include_str!("listing.rs").split(concat!("#[cfg", "(test)]")).next().unwrap()
    }

    #[test]
    fn details_sits_in_the_right_column_under_the_download_link_and_the_studio_hint() {
        let page = page_source();
        let at = |needle: &str| page.find(needle).unwrap_or_else(|| panic!("{needle} is not in the page"));
        let download = at("\"Download Eustress Player\"");
        let studio_hint = at("class=\"g-hint g-studio-hint\"");
        let details = at("<h2>\"Details\"</h2>");
        let side_end = at("</aside>");
        let copy = at("<div class=\"g-lcopy\">");
        assert!(download < details && studio_hint < details, "Details comes after the download link and the open-source hint");
        assert!(details < side_end && side_end < copy, "Details is in the right column, not in the left one");
        assert_eq!(page.matches("<h2>\"Details\"</h2>").count(), 1, "Details is drawn once");
        assert!(!page.contains("<h2>\"About\"</h2>"), "the short description has no heading");
    }

    #[test]
    fn the_four_tabs_are_named_and_drawn_as_a_tab_list() {
        let labels: Vec<&str> = Tab::ALL.iter().map(|t| t.label()).collect();
        assert_eq!(labels, ["Servers", "Shop", "Links", "About"]);
        let page = page_source();
        for needle in [
            "role=\"tablist\"",
            "role=\"tab\"",
            "role=\"tabpanel\"",
            "{tab.label()}",
            "aria-selected=",
            "aria-controls=format!(\"lt-panel-{}\", tab.key())",
            "id=\"lt-panel-servers\"",
            "id=\"lt-panel-shop\"",
            "id=\"lt-panel-links\"",
            "id=\"lt-panel-about\"",
            "<PassStore sim_id=shop_id />",
            "readme_view(&readme,",
            "links_view(&page.get_untracked())",
        ] {
            assert!(page.contains(needle), "{needle} is not in the page");
        }
        // The live status moved into the Servers panel, the Offline line and Join with it.
        let servers = at_between(page, "id=\"lt-panel-servers\"", "id=\"lt-panel-shop\"");
        assert!(servers.contains("\"Offline: nobody is hosting right now\"") && servers.contains(">\"Join\"</button>"), "{servers}");
    }

    fn at_between<'a>(page: &'a str, from: &str, to: &str) -> &'a str {
        let a = page.find(from).expect("start");
        let b = a + page[a..].find(to).expect("end");
        &page[a..b]
    }

    #[test]
    fn links_and_about_show_only_with_something_in_them_and_about_comes_up_first() {
        assert_eq!(visible_tabs(false, false), [Tab::Servers, Tab::Shop]);
        assert_eq!(visible_tabs(true, false), [Tab::Servers, Tab::Shop, Tab::Links]);
        assert_eq!(visible_tabs(false, true), [Tab::Servers, Tab::Shop, Tab::About]);
        assert_eq!(visible_tabs(true, true), Tab::ALL);
        // Before the visitor chooses: About when it is there, else Servers. The README arriving later moves an unchosen page to About.
        assert_eq!(shown_tab(None, &visible_tabs(true, false)), Tab::Servers);
        assert_eq!(shown_tab(None, &visible_tabs(false, true)), Tab::About);
        // A choice holds while its tab is shown, including after the README arrives.
        assert_eq!(shown_tab(Some(Tab::Shop), &visible_tabs(false, false)), Tab::Shop);
        assert_eq!(shown_tab(Some(Tab::Shop), &visible_tabs(false, true)), Tab::Shop);
        assert_eq!(shown_tab(Some(Tab::Links), &visible_tabs(false, false)), Tab::Servers, "a hidden tab is never up");
        assert_eq!(shown_tab(Some(Tab::About), &visible_tabs(false, false)), Tab::Servers);
    }

    #[test]
    fn the_arrow_keys_move_through_the_shown_tabs_and_wrap() {
        let v = visible_tabs(false, true);
        assert_eq!(key_tab(Tab::Servers, &v, "ArrowRight"), Some(Tab::Shop));
        assert_eq!(key_tab(Tab::Shop, &v, "ArrowRight"), Some(Tab::About), "Links is hidden, so it is skipped");
        assert_eq!(key_tab(Tab::About, &v, "ArrowRight"), Some(Tab::Servers));
        assert_eq!(key_tab(Tab::Servers, &v, "ArrowLeft"), Some(Tab::About));
        assert_eq!(key_tab(Tab::Shop, &v, "Home"), Some(Tab::Servers));
        assert_eq!(key_tab(Tab::Servers, &v, "End"), Some(Tab::About));
        assert_eq!(key_tab(Tab::Servers, &v, "Enter"), None);
        assert_eq!(key_tab(Tab::Servers, &[], "ArrowRight"), None);
    }

    #[test]
    fn the_tabs_follow_what_the_page_has() {
        use crate::api::sim_page::{PageLink, Readme};
        let mut page = SimPage::default();
        assert!(!page_has_links(&Some(page.clone())) && !page_has_readme(&Some(page.clone())));
        assert!(!page_has_links(&None) && !page_has_readme(&None));
        // A link that would not be drawn does not make a Links tab.
        page.links = vec![PageLink { kind: "website".into(), url: "http://insecure.example".into(), label: "Site".into() }];
        assert!(!page_has_links(&Some(page.clone())));
        page.links.push(PageLink { kind: "website".into(), url: "https://example.com".into(), label: "Site".into() });
        assert!(page_has_links(&Some(page.clone())));
        // A README that draws nothing (only raw HTML) makes no About tab.
        page.readme = Some(Readme { markdown: "<div>hidden</div>".into(), ..Default::default() });
        assert!(!page_has_readme(&Some(page.clone())));
        page.readme = Some(Readme { markdown: "# Battery lab".into(), ..Default::default() });
        assert!(page_has_readme(&Some(page)));
    }

    #[test]
    fn the_crumb_names_the_first_tag_and_a_genre_in_the_answer_is_no_crumb() {
        let mut l = listing();
        assert_eq!(crumb_tag(&l), None);
        let old: Listing = serde_json::from_str("{\"id\":\"a\",\"genre\":\"Horror\"}").unwrap();
        assert_eq!(crumb_tag(&old), None, "an older answer with only a genre has no crumb");
        l.tags = vec!["racing".to_string(), "co-op".to_string()];
        assert_eq!(crumb_tag(&l), Some("racing".to_string()));
    }
}
