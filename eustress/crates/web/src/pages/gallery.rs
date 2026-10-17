// =============================================================================
// Eustress Web - The Gallery
// =============================================================================
// What is published and playable, and who is playing it right now. Every number and every picture is something the Worker said:
//
//   GET /api/simulations           the listed Spaces, newest first
//   GET /api/simulations/live      the live sessions, with the number of players in them
//   GET /api/simulations/tags      the tags in use
//
// A Space shows its creator's cover when it has one and otherwise the capture Studio took at publish; hovering or focusing a card
// plays the captures taken at publish (components/listing_art.rs). Every Play button opens the Play chooser in place
// (components/play_chooser.rs). The search pill and Ctrl+K open the search over Spaces, makers and tags
// (components/command_palette.rs). A route this deployment lacks reads as nothing: no invented rows, no placeholder numbers.
// Tags in the address (`/gallery?tag=horror&tag=co-op`) filter the page to the Spaces that carry every one of them. An old
// `?genre=horror` link is read as `?tag=horror`, and the address is rewritten to say so.
//
// Table of Contents:
// 1. Choosing and wording what is shown (pure, tested)
// 2. The page
// 3. The lead Space
// 4. Cards
// 5. Loading, failing, empty
// =============================================================================

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::catalog::{self, card_tags, is_multiplayer, is_recent, lead_listings, published_words, rating_words, region_words, size_words, Listing, LiveNow, Session, TagCount};
use crate::api::{self, ApiClient};
use crate::components::command_palette::SearchPill;
use crate::components::listing_art::ListingArt;
use crate::components::play_chooser::{now_ms, PlayChooserSlot};
use crate::components::{CentralNav, Footer};
use crate::state::AppState;
use crate::tags::{gallery_href, normalize_tag, tags_in_address, toggled};

/// How often the live sessions are asked again while the page is open.
const LIVE_REFRESH_MS: u32 = 20_000;
/// How long the lead Space stays up before the next one, when there are several.
const LEAD_MS: u32 = 8_000;
/// How many Spaces may lead.
const LEADS: usize = 3;
/// "New" means published within this many days.
const NEW_DAYS: f64 = 14.0;
/// A Space published within this many days wears a "New" badge.
const BADGE_DAYS: f64 = 2.0;
/// How many tags the filter row offers.
const TAG_CHIPS: usize = 8;

// -----------------------------------------------------------------------------
// 1. Choosing and wording what is shown
// -----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    New,
    Multiplayer,
    Solo,
}

impl Filter {
    pub const ALL: [Filter; 4] = [Filter::All, Filter::New, Filter::Multiplayer, Filter::Solo];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::New => "New",
            Filter::Multiplayer => "Multiplayer",
            Filter::Solo => "Solo",
        }
    }
}

/// A tag from the address, if it is one the Worker would accept (src/tags.rs): lower-case words joined by single hyphens, 2 to 24
/// characters. Anything else is ignored rather than shown, so a hand-typed address cannot put markup or a long string on the page.
pub fn clean_tag(raw: &str) -> Option<String> {
    normalize_tag(raw)
}

/// The value of an old `?genre=` in the address, if the link has one. It is read for one reason: to become a tag.
fn legacy_genre_param(q: &leptos_router::params::ParamsMap) -> Option<String> {
    q.get("genre")
}

/// The Spaces the page shows: those that pass the filter and carry every one of the tags. The Worker's order, newest first, is kept.
pub fn apply_filters(listings: &[Listing], filter: Filter, tags: &[String], now_ms: f64) -> Vec<Listing> {
    listings
        .iter()
        .filter(|l| match filter {
            Filter::All => true,
            Filter::New => is_recent(l, NEW_DAYS, now_ms),
            Filter::Multiplayer => is_multiplayer(l),
            Filter::Solo => !is_multiplayer(l),
        })
        .filter(|l| tags.iter().all(|t| card_tags(l).iter().any(|c| c == t)))
        .cloned()
        .collect()
}

/// The tags as the heading writes them: `#horror`, or `#horror #co-op`.
fn tags_words(tags: &[String]) -> String {
    tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ")
}

/// The heading over the grid: what the filter is, never a claim about ranking the page does not do.
pub fn shelf_title(filter: Filter, tags: &[String]) -> String {
    match (tags.is_empty(), filter) {
        (false, Filter::All) => tags_words(tags),
        (false, f) => format!("{} \u{00b7} {}", tags_words(tags), f.label()),
        (true, Filter::All) => "All Spaces".to_string(),
        (true, Filter::New) => "New in the last two weeks".to_string(),
        (true, Filter::Multiplayer) => "Multiplayer".to_string(),
        (true, Filter::Solo) => "Solo".to_string(),
    }
}

/// What the grid says when nothing passes: a headline and the way forward.
pub fn nothing_here(filter: Filter, tags: &[String]) -> (String, &'static str) {
    match (tags.len(), filter) {
        (1, _) => (format!("No Space uses #{} yet", tags[0]), "Clear the tag to see everything, or search for something else."),
        (n, _) if n > 1 => (format!("No Space uses {} together yet", tags_words(tags)), "Clear a tag to see more, or search for something else."),
        (_, Filter::New) => ("Nothing new in the last two weeks".to_string(), "Show everything to see what is published."),
        (_, Filter::Multiplayer) => ("No multiplayer Spaces yet".to_string(), "Show everything to see what is published."),
        (_, Filter::Solo) => ("No solo Spaces yet".to_string(), "Show everything to see what is published."),
        (_, Filter::All) => ("Nothing is published yet".to_string(), "Spaces appear here once they are approved."),
    }
}

/// The small label over the lead Space, from what is true of it.
pub fn lead_kicker(listing: &Listing, now_ms: f64) -> &'static str {
    if listing.featured {
        "Featured"
    } else if is_recent(listing, 7.0, now_ms) {
        "New this week"
    } else {
        "In the Gallery"
    }
}

/// The live session of a Space, if one is up.
pub fn session_of<'a>(live: &'a LiveNow, id: &str) -> Option<&'a Session> {
    live.sessions.iter().find(|s| s.id == id)
}

/// The next Space to lead, wrapping at the end.
pub fn next_lead(current: usize, count: usize) -> usize {
    if count == 0 { 0 } else { (current + 1) % count }
}

/// "2/4": players in a session out of the room it has.
pub fn seats_words(players: u32, max_players: u32) -> String {
    if max_players == 0 { players.to_string() } else { format!("{players}/{max_players}") }
}

/// What a card's corner says about who can play: the seats of a live session, otherwise the room the author set.
pub fn corner_words(listing: &Listing, session: Option<&Session>) -> String {
    match session {
        Some(s) => seats_words(s.players, s.max_players),
        None if is_multiplayer(listing) => format!("up to {}", listing.max_players),
        None => "Solo".to_string(),
    }
}

/// "3 players", "1 player".
pub fn players_words(n: u32) -> String {
    if n == 1 { "1 player".to_string() } else { format!("{n} players") }
}

/// "today", "yesterday", "3 days ago" for the last update, left out when it reads the same as the publish day (nothing to add) or is missing.
pub fn updated_label(updated_at: Option<&str>, published_label: Option<&str>, now_ms: f64) -> Option<String> {
    let label = published_words(updated_at?, now_ms)?.trim_start_matches("published ").to_string();
    (Some(label.as_str()) != published_label).then_some(label)
}

/// The line over the live rail's total: how many sessions the players are in.
pub fn sessions_words(n: u32) -> String {
    if n == 1 { "1 live session".to_string() } else { format!("{n} live sessions") }
}

fn display_name(listing: &Listing) -> String {
    if listing.name.trim().is_empty() { "Untitled Space".to_string() } else { listing.name.clone() }
}

fn author_of(listing: &Listing) -> Option<String> {
    listing.author_name.clone().filter(|n| !n.trim().is_empty())
}

// -----------------------------------------------------------------------------
// 2. The page
// -----------------------------------------------------------------------------

/// `/simulations` was a page of invented Spaces; the Gallery is the one list.
#[component]
pub fn SimulationsRedirect() -> impl IntoView {
    view! { <leptos_router::components::Redirect path="/gallery" options=leptos_router::NavigateOptions { replace: true, ..Default::default() } /> }
}

#[derive(Clone, Debug, PartialEq)]
enum Shelf {
    Loading,
    Ready(Vec<Listing>),
    Failed(String),
}

const PLAY_ICON: &str = "M8 5.5v13l11-6.5z";

#[component]
pub fn GalleryPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let query = leptos_router::hooks::use_query_map();
    // Every valid `tag=` in the address, then an old `genre=` as a tag.
    let picked = Memo::new(move |_| {
        let q = query.read();
        tags_in_address(&q.get_all("tag").unwrap_or_default(), legacy_genre_param(&q).as_deref())
    });
    // An old link, `/gallery?genre=horror`, shows `/gallery?tag=horror`: the address is rewritten once, without a history entry.
    Effect::new(move |_| {
        if legacy_genre_param(&query.read()).is_some() {
            crate::utils::replace_path(&gallery_href(&picked.get()));
        }
    });

    let shelf = RwSignal::new(Shelf::Loading);
    let live = RwSignal::new(LiveNow::default());
    let tags = RwSignal::new(Vec::<TagCount>::new());
    let filter = RwSignal::new(Filter::All);
    let runs = StoredValue::new(0u64);

    let load = move || {
        shelf.set(Shelf::Loading);
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            match catalog::fetch_listings(&client).await {
                Ok(list) => shelf.set(Shelf::Ready(list)),
                Err(e) => shelf.set(Shelf::Failed(api::failure_sentence(&e))),
            }
        });
    };

    // In the browser only: the Spaces, the tags in use once, and the live sessions again every little while.
    Effect::new(move |_| {
        load();
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            tags.set(catalog::fetch_popular_tags(&client).await);
        });
        let run = runs.get_value() + 1;
        runs.set_value(run);
        spawn_local(async move {
            loop {
                let client = ApiClient::new(&api_url.get_value());
                let now = catalog::fetch_live_now(&client).await;
                if runs.try_get_value() != Some(run) {
                    return;
                }
                live.set(now);
                TimeoutFuture::new(LIVE_REFRESH_MS).await;
                if runs.try_get_value() != Some(run) {
                    return;
                }
            }
        });
    });

    view! {
        <div class="page g-page">
            <CentralNav active="gallery".to_string() />
            <div class="g-backdrop" aria-hidden="true"></div>
            <main class="g-main">
                <h1 class="g-sr">"Gallery"</h1>
                {move || match shelf.get() {
                    Shelf::Loading => view! { <GallerySkeleton /> }.into_any(),
                    Shelf::Failed(why) => view! { <GalleryFailed why=why retry=Callback::new(move |_| load()) /> }.into_any(),
                    Shelf::Ready(list) if list.is_empty() => view! { <GalleryEmpty /> }.into_any(),
                    Shelf::Ready(list) => view! { <Loaded list=list live=live tags=tags filter=filter picked=picked /> }.into_any(),
                }}
            </main>
            <Footer />
        </div>
    }
}

#[component]
fn Loaded(list: Vec<Listing>, live: RwSignal<LiveNow>, tags: RwSignal<Vec<TagCount>>, filter: RwSignal<Filter>, picked: Memo<Vec<String>>) -> impl IntoView {
    let list = StoredValue::new(list);
    let leading = move || picked.get().is_empty() && filter.get() == Filter::All;
    let shown = move || list.with_value(|all| apply_filters(all, filter.get(), &picked.get(), now_ms()));

    // The tags to offer: the ones in use, and any being filtered that are not among them.
    let tag_chips = move || {
        let mut chips: Vec<String> = tags.get().into_iter().map(|t| t.tag).take(TAG_CHIPS).collect();
        for t in picked.get().into_iter().rev() {
            if !chips.contains(&t) {
                chips.insert(0, t);
            }
        }
        chips
    };

    view! {
        {move || leading().then(|| view! { <Lead listings=list.with_value(|all| lead_listings(all, LEADS)) live=live /> })}

        <div class="g-wrap">
            // The players in Eustress right now: nothing at all when it is zero.
            {move || {
                let now = live.get();
                (now.players_total > 0).then(|| view! {
                    <div class="g-glass g-strip" role="status">
                        <span class="g-dot" aria-hidden="true"></span>
                        <b>{format!("{} in Eustress right now", players_words(now.players_total))}</b>
                        <span class="g-hint">{sessions_words(now.sessions_total.max(now.sessions.len() as u32))}</span>
                    </div>
                })
            }}

            <div class="g-search-row"><SearchPill /></div>

            <div class="g-filters" role="group" aria-label="Filter Spaces">
                {Filter::ALL.iter().map(|f| {
                    let f = *f;
                    view! {
                        <button type="button" class="g-chip g-chip-btn" class:on=move || filter.get() == f aria-pressed=move || (filter.get() == f).to_string() on:click=move |_| filter.set(f)>
                            {f.label()}
                        </button>
                    }
                }).collect_view()}
                <span class="g-sep" aria-hidden="true"></span>
                {move || {
                    let chips = tag_chips();
                    if chips.is_empty() {
                        view! { <span class="g-hint">"Tags appear here as creators add them."</span> }.into_any()
                    } else {
                        let selected = picked.get();
                        chips.into_iter().map(|t| {
                            let on = selected.contains(&t);
                            // A tag that is on is a link that takes it off; one that is off adds itself, and the Spaces must carry every tag.
                            let href = gallery_href(&toggled(&selected, &t));
                            view! {
                                <a class="g-chip g-tag" class:on=on href=href aria-current=if on { Some("true") } else { None }>
                                    {t}{on.then(|| view! { <span class="g-chip-x" aria-label="Clear this tag">"\u{00d7}"</span> })}
                                </a>
                            }
                        }).collect_view().into_any()
                    }
                }}
            </div>

            // Live now: real sessions only; when there are none it says so.
            {move || leading().then(|| view! {
                <section class="g-section" aria-labelledby="g-live-h">
                    <div class="g-head"><h2 id="g-live-h">"Live now"</h2><span class="g-rule"></span></div>
                    {move || {
                        let sessions = live.get().sessions;
                        if sessions.is_empty() {
                            view! {
                                <div class="g-glass g-empty-live">
                                    <span class="g-glyph" aria-hidden="true"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M9 9l6 6M15 9l-6 6"/></svg></span>
                                    <div><b>"Nobody is hosting right now"</b><p>"Live sessions appear here the moment a host goes live."</p></div>
                                </div>
                            }.into_any()
                        } else {
                            view! { <div class="g-rail">{sessions.into_iter().map(|s| view! { <LiveCard session=s /> }).collect_view()}</div> }.into_any()
                        }
                    }}
                </section>
            })}

            <section class="g-section" aria-labelledby="g-grid-h">
                <div class="g-head"><h2 id="g-grid-h">{move || shelf_title(filter.get(), &picked.get())}</h2><span class="g-rule"></span></div>
                {move || {
                    let rows = shown();
                    if rows.is_empty() {
                        let (headline, hint) = nothing_here(filter.get(), &picked.get());
                        view! {
                            <div class="g-glass g-none">
                                <b>{headline}</b>
                                <p>{hint}</p>
                                <div class="g-actions">
                                    <a class="g-btn" href="/gallery" on:click=move |_| filter.set(Filter::All)>"Show everything"</a>
                                </div>
                            </div>
                        }.into_any()
                    } else {
                        view! {
                            <div class="g-grid">
                                <For each=move || shown() key=|l| l.id.clone() children=move |l| view! { <ListingCard listing=l live=live /> } />
                                <a class="g-card g-make" href="/download">
                                    <div>
                                        <h3>"Your Space could be here"</h3>
                                        <p>"Build it in Eustress Studio and publish it. It appears in this grid once it is approved."</p>
                                        <span class="g-btn">"Download Studio"</span>
                                    </div>
                                </a>
                            </div>
                        }.into_any()
                    }
                }}
            </section>

            <div class="g-glass g-band">
                <div>
                    <h2>"Build it in Eustress Studio"</h2>
                    <p>"Model a real system, script it in Luau or Rune, and publish it from Studio. Players open it in Eustress Player."</p>
                </div>
                <div class="g-actions">
                    <a class="g-btn g-btn-primary g-btn-lg" href="/download">"Download Studio"</a>
                    <a class="g-btn g-btn-lg" href="/docs/publishing">"How publishing works"</a>
                </div>
            </div>
        </div>
    }
}

// -----------------------------------------------------------------------------
// 3. The lead Space
// -----------------------------------------------------------------------------

/// The Space the page leads with: the featured ones first, then the most visited. One Space stays; with several, they take turns,
/// except for a reader who asked for less motion, and for as long as the pointer or the focus is on it or Pause is pressed.
#[component]
fn Lead(listings: Vec<Listing>, live: RwSignal<LiveNow>) -> impl IntoView {
    let chooser = expect_context::<PlayChooserSlot>();
    let count = listings.len();
    let listings = StoredValue::new(listings);
    let index = RwSignal::new(0usize);
    let hovering = RwSignal::new(false);
    let paused = RwSignal::new(false);
    let runs = StoredValue::new(0u64);

    if count > 1 {
        Effect::new(move |_| {
            if crate::utils::prefers_reduced_motion() {
                return;
            }
            let run = runs.get_value() + 1;
            runs.set_value(run);
            spawn_local(async move {
                loop {
                    TimeoutFuture::new(LEAD_MS).await;
                    if runs.try_get_value() != Some(run) {
                        return;
                    }
                    if !hovering.get_untracked() && !paused.get_untracked() {
                        index.update(|i| *i = next_lead(*i, count));
                    }
                }
            });
        });
    }

    view! {
        <section class="g-hero" aria-label="Featured Space" on:mouseenter=move |_| hovering.set(true) on:mouseleave=move |_| hovering.set(false)
            on:focusin=move |_| hovering.set(true) on:focusout=move |_| hovering.set(false)>
            <div class="g-wrap g-hero-grid">
                {move || {
                    let l = listings.with_value(|all| all[index.get() % all.len()].clone());
                    let name = display_name(&l);
                    let now = now_ms();
                    let kicker = lead_kicker(&l, now);
                    let pitch = l.description.trim().to_string();
                    let tag_links = card_tags(&l).into_iter().take(3).map(|t| {
                        let href = gallery_href(std::slice::from_ref(&t));
                        view! { <a class="g-chip g-tag" href=href>{t}</a> }
                    }).collect_view();
                    let rating = rating_words(l.rating.as_deref());
                    let room = if is_multiplayer(&l) { format!("Up to {} players", l.max_players) } else { "Solo".to_string() };
                    let author = author_of(&l);
                    let author_href = author.as_ref().map(|a| format!("/profile/{}", urlencoding::encode(a)));
                    let details = format!("/simulation/{}", urlencoding::encode(&l.id));
                    let id = l.id.clone();
                    let name_for_play = name.clone();
                    let initial = author.as_ref().and_then(|a| a.chars().next()).map(|c| c.to_uppercase().to_string()).unwrap_or_default();
                    let size = l.scene_size_bytes.map(size_words);
                    let published = l.published_at.as_deref().and_then(|iso| published_words(iso, now)).map(|w| w.trim_start_matches("published ").to_string());
                    let session = live.with(|n| session_of(n, &l.id).cloned());
                    let spec: Vec<(&'static str, String)> = [
                        Some(("Players", if is_multiplayer(&l) { format!("Up to {}", l.max_players) } else { "Solo".to_string() })),
                        size.map(|s| ("Size", s)),
                        published.clone().map(|p| ("Published", p)),
                        updated_label(l.updated_at.as_deref(), published.as_deref(), now).map(|u| ("Updated", u)),
                        Some(("Visits", l.play_count.to_string())),
                    ].into_iter().flatten().collect();
                    view! {
                        <div class="g-hero-copy">
                            <div class="g-kicker"><span class="g-kick-rule"></span>{kicker}</div>
                            <h2 class="g-hero-title">{name.clone()}</h2>
                            {(!pitch.is_empty()).then(|| view! { <p class="g-pitch">{pitch}</p> })}
                            <div class="g-chips">
                                {tag_links}
                                {rating.map(|r| view! { <span class="g-chip">{r}</span> })}
                                <span class="g-chip">{room}</span>
                                {session.map(|s| view! { <span class="g-chip live"><i class="g-dot"></i>{format!("Live {}", seats_words(s.players, s.max_players))}</span> })}
                            </div>
                            <div class="g-cta">
                                <button type="button" class="g-btn g-btn-primary g-btn-lg" on:click=move |_| chooser.open(&id)>
                                    <svg viewBox="0 0 24 24" aria-hidden="true"><path d=PLAY_ICON/></svg>
                                    <span>"Play"</span>
                                    <span class="g-sr">{format!(" {name_for_play}")}</span>
                                </button>
                                <a class="g-btn g-btn-lg" href=details>"Details"</a>
                                {author.zip(author_href).map(|(a, href)| view! {
                                    <a class="g-maker" href=href><span class="g-av" aria-hidden="true">{initial}</span>{format!("by @{a}")}</a>
                                })}
                            </div>
                            <dl class="g-spec">
                                {spec.into_iter().map(|(k, v)| view! { <div><dt>{k}</dt><dd>{v}</dd></div> }).collect_view()}
                            </dl>
                        </div>
                        <div class="g-hero-media">
                            <div class="g-frame">
                                <span class="g-bracket tl"></span><span class="g-bracket tr"></span><span class="g-bracket bl"></span><span class="g-bracket br"></span>
                                <ListingArt listing=l.clone() active=Signal::derive(move || true) eager=true />
                            </div>
                        </div>
                    }
                }}
            </div>
            {(count > 1).then(|| view! {
                <div class="g-wrap g-lead-nav">
                    <div class="g-dots" role="group" aria-label="Choose a featured Space">
                        {(0..count).map(|i| view! {
                            <button type="button" class="g-dotbtn" class:on=move || index.get() == i aria-label=format!("Show Space {} of {}", i + 1, count) aria-current=move || if index.get() == i { Some("true") } else { None } on:click=move |_| index.set(i)></button>
                        }).collect_view()}
                    </div>
                    <button type="button" class="g-pause" aria-pressed=move || paused.get().to_string() on:click=move |_| paused.update(|p| *p = !*p)>
                        {move || if paused.get() { "Resume" } else { "Pause" }}
                    </button>
                </div>
            })}
        </section>
    }
}

// -----------------------------------------------------------------------------
// 4. Cards
// -----------------------------------------------------------------------------

/// A Space in the grid. The whole card opens its page; the Play button opens the chooser; the tags and the author are links of
/// their own. Hover or focus plays the captures taken at publish.
#[component]
pub(crate) fn ListingCard(listing: Listing, live: RwSignal<LiveNow>) -> impl IntoView {
    let chooser = expect_context::<PlayChooserSlot>();
    let hot = RwSignal::new(false);
    let id = listing.id.clone();
    let session = {
        let id = id.clone();
        Memo::new(move |_| live.with(|n| session_of(n, &id).cloned()))
    };
    let now = now_ms();
    let name = display_name(&listing);
    let href = format!("/simulation/{}", urlencoding::encode(&listing.id));
    let author = author_of(&listing);
    let author_href = author.as_ref().map(|a| format!("/profile/{}", urlencoding::encode(a)));
    let tags = card_tags(&listing).into_iter().take(2).collect::<Vec<_>>();
    let rating = rating_words(listing.rating.as_deref());
    let multiplayer = is_multiplayer(&listing);
    let fresh = is_recent(&listing, BADGE_DAYS, now);
    let size = listing.scene_size_bytes.map(size_words);
    let published = listing.published_at.as_deref().and_then(|iso| published_words(iso, now));
    let visits = listing.play_count;
    let corner_listing = listing.clone();
    let play_id = id.clone();
    let play_label = format!("Play {name}");

    view! {
        <article class="g-card" class:hot=move || hot.get()
            on:mouseenter=move |_| hot.set(true) on:mouseleave=move |_| hot.set(false)
            on:focusin=move |_| hot.set(true) on:focusout=move |_| hot.set(false)>
            <div class="g-media">
                <ListingArt listing=listing active=hot />
                {move || match session.get() {
                    Some(_) => view! { <span class="g-chip live g-badge"><i class="g-dot"></i>"Live"</span> }.into_any(),
                    None if fresh => view! { <span class="g-chip g-badge">"New"</span> }.into_any(),
                    None => ().into_any(),
                }}
                <span class="g-count">{move || corner_words(&corner_listing, session.get().as_ref())}</span>
                <button type="button" class="g-play" aria-label=play_label on:click=move |_| chooser.open(&play_id)>
                    <svg viewBox="0 0 24 24" aria-hidden="true"><path d=PLAY_ICON/></svg>
                </button>
            </div>
            <div class="g-body">
                <h3 class="g-name"><a class="g-link" href=href>{name}</a></h3>
                {author.zip(author_href).map(|(a, href)| view! { <div class="g-by">"by "<a href=href>{format!("@{a}")}</a></div> })}
                <div class="g-chips">
                    {tags.into_iter().map(|t| {
                        let href = gallery_href(std::slice::from_ref(&t));
                        view! { <a class="g-chip g-tag" href=href>{t}</a> }
                    }).collect_view()}
                    <span class="g-chip">{if multiplayer { "Multiplayer" } else { "Solo" }}</span>
                    {rating.map(|r| view! { <span class="g-chip">{r}</span> })}
                </div>
                <div class="g-facts">
                    <span><b>{visits}</b>{if visits == 1 { " visit" } else { " visits" }}</span>
                    {size.map(|s| view! { <span>{s}</span> })}
                    {published.map(|p| view! { <span>{p.replace("published ", "")}</span> })}
                </div>
            </div>
        </article>
    }
}

/// A live session's art in the rail, or nothing when it has none. The Worker answers 404 for art it will not serve (animated,
/// transparent, several images, not an image, too large), so a picture that fails to load clears art_ok and the box is left
/// empty, as for a session with no art, never the browser's broken-image icon. Each card makes its own signal.
fn live_picture(art: Option<String>, art_ok: RwSignal<bool>) -> impl IntoView {
    move || art.clone().filter(|_| art_ok.get()).map(|u| view! { <img src=u alt="" loading="lazy" decoding="async" on:error=move |_| art_ok.set(false) /> })
}

/// A live session in the rail: the Space, who hosts it, where, how full it is, and Join (which opens the chooser).
#[component]
pub(crate) fn LiveCard(session: Session) -> impl IntoView {
    let chooser = expect_context::<PlayChooserSlot>();
    let name = if session.name.trim().is_empty() { "Untitled Space".to_string() } else { session.name.clone() };
    let art = session.cover_url.clone().or_else(|| session.thumbnail_url.clone()).filter(|u| u.starts_with("https://") || u.starts_with('/'));
    let by = session.author_name.clone().filter(|n| !n.trim().is_empty()).map(|n| format!("@{n}"));
    let region = region_words(session.region.as_ref());
    let line = [by, region].into_iter().flatten().collect::<Vec<_>>().join(" \u{00b7} ");
    let seats = seats_words(session.players, session.max_players);
    let id = session.id.clone();
    let href = format!("/simulation/{}", urlencoding::encode(&session.id));
    let label = format!("Join {name}");
    view! {
        <div class="g-glass g-live-card">
            <a class="g-live-art" href=href.clone() tabindex="-1" aria-hidden="true">{live_picture(art, RwSignal::new(true))}</a>
            <div class="g-live-text">
                <b><a href=href>{name}</a></b>
                <small>{line}</small>
            </div>
            <div class="g-live-side">
                <span class="g-chip live"><i class="g-dot"></i>{seats}</span>
                <button type="button" class="g-btn g-btn-primary g-btn-sm" aria-label=label on:click=move |_| chooser.open(&id)>"Join"</button>
            </div>
        </div>
    }
}

// -----------------------------------------------------------------------------
// 5. Loading, failing, empty
// -----------------------------------------------------------------------------

/// What the page looks like while the Spaces load: the shapes of the page, not made-up content.
#[component]
fn GallerySkeleton() -> impl IntoView {
    view! {
        <div class="g-wrap g-skeleton" role="status" aria-busy="true" aria-label="Loading the Gallery">
            <div class="g-hero-grid g-skel-hero">
                <div class="g-skel-copy"><span class="g-skel w30"></span><span class="g-skel w80 tall"></span><span class="g-skel w60"></span><span class="g-skel w40 btn"></span></div>
                <div class="g-skel g-skel-frame"></div>
            </div>
            <div class="g-grid">
                {(0..6).map(|_| view! { <div class="g-card g-skel-card"><div class="g-skel g-skel-media"></div><div class="g-body"><span class="g-skel w70"></span><span class="g-skel w40"></span></div></div> }).collect_view()}
            </div>
        </div>
    }
}

#[component]
fn GalleryFailed(why: String, retry: Callback<()>) -> impl IntoView {
    view! {
        <div class="g-wrap">
            <div class="g-glass g-none" role="alert">
                <b>"The Gallery could not load"</b>
                <p>{why}</p>
                <div class="g-actions"><button type="button" class="g-btn g-btn-primary" on:click=move |_| retry.run(())>"Try again"</button></div>
            </div>
        </div>
    }
}

#[component]
fn GalleryEmpty() -> impl IntoView {
    view! {
        <div class="g-wrap">
            <div class="g-glass g-none">
                <b>"Nothing is published yet"</b>
                <p>"Spaces appear here once they are approved. Build one in Eustress Studio and publish it."</p>
                <div class="g-actions"><a class="g-btn g-btn-primary" href="/download">"Download Studio"</a></div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_791_000_000_000.0; // 2026-10-03T03:20:00Z
    const DAY: f64 = 86_400_000.0;

    fn iso(days_ago: f64) -> String {
        chrono::DateTime::from_timestamp_millis((NOW - days_ago * DAY) as i64).unwrap().to_rfc3339()
    }

    fn space(id: &str, max: u32, days_ago: f64, tags: &[&str]) -> Listing {
        Listing {
            id: id.to_string(),
            name: id.to_string(),
            max_players: max,
            published_at: Some(iso(days_ago)),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            ..Default::default()
        }
    }

    fn ids(rows: &[Listing]) -> Vec<&str> {
        rows.iter().map(|l| l.id.as_str()).collect()
    }

    /// What a live card's art box draws for this picture, rendered on the server. art_ok is the signal the img's on:error clears.
    fn art_html(art: Option<&str>, art_ok: bool) -> String {
        use leptos::tachys::view::RenderHtml;
        let owner = Owner::new();
        owner.with(|| live_picture(art.map(str::to_string), RwSignal::new(art_ok)).to_html())
    }

    #[test]
    fn a_live_card_picture_that_will_not_load_leaves_the_empty_box_a_card_without_one_has() {
        let url = "https://api.example.dev/a/thumbnail";
        let shown = art_html(Some(url), true);
        assert!(shown.contains("<img") && shown.contains(url), "{shown}");
        let none = art_html(None, true);
        assert!(!none.contains("<img"), "{none}");
        // art_ok is false once the img's on:error has run: the card then draws exactly what it draws for a session with no art.
        let failed = art_html(Some(url), false);
        assert_eq!(failed, none, "a picture that failed leaves the box empty, not the browser's broken-image icon");
        assert!(!failed.contains("<img") && !failed.contains(url));
    }

    #[test]
    fn the_live_card_picture_has_an_error_handler_that_clears_its_own_signal() {
        let page = include_str!("gallery.rs").split(concat!("#[cfg", "(test)]")).next().unwrap();
        assert_eq!(page.matches("src=u alt=\"\"").count(), 1, "one picture tag in the live rail");
        let at = page.find("src=u alt=\"\"").unwrap();
        let start = page[..at].rfind("<img").expect("an img tag");
        let tag = &page[start..start + page[start..].find("/>").expect("the tag ends")];
        assert!(tag.contains("on:error=move |_| art_ok.set(false)"), "{tag}");
        assert!(page.contains("art.clone().filter(|_| art_ok.get())"), "the picture is drawn only while art_ok");
        // One signal for each card: a signal shared by the cards would blank every card at the first failure.
        assert!(page.contains("{live_picture(art, RwSignal::new(true))}</a>"));
    }

    #[test]
    fn a_tag_from_the_address_is_used_only_when_the_worker_would_accept_it() {
        assert_eq!(clean_tag("horror"), Some("horror".to_string()));
        assert_eq!(clean_tag("#Co-Op"), Some("co-op".to_string()));
        assert_eq!(clean_tag("a"), None, "one character is too short");
        assert_eq!(clean_tag("<script>"), None);
        assert_eq!(clean_tag("two words"), None);
        assert_eq!(clean_tag("-lead"), None);
        assert_eq!(clean_tag("double--hyphen"), None);
        assert_eq!(clean_tag(&"a".repeat(25)), None);
        assert_eq!(clean_tag(&"a".repeat(24)).map(|t| t.len()), Some(24));
        assert_eq!(clean_tag(""), None);
    }

    #[test]
    fn each_filter_keeps_what_its_name_says_and_the_workers_order() {
        let all = [space("old-solo", 1, 40.0, &[]), space("new-duo", 2, 3.0, &["racing"]), space("new-solo", 1, 1.0, &["racing"]), space("old-big", 8, 20.0, &["horror"])];
        let none: &[String] = &[];
        let racing = [String::from("racing")];
        assert_eq!(ids(&apply_filters(&all, Filter::All, none, NOW)), ["old-solo", "new-duo", "new-solo", "old-big"]);
        assert_eq!(ids(&apply_filters(&all, Filter::New, none, NOW)), ["new-duo", "new-solo"]);
        assert_eq!(ids(&apply_filters(&all, Filter::Multiplayer, none, NOW)), ["new-duo", "old-big"]);
        assert_eq!(ids(&apply_filters(&all, Filter::Solo, none, NOW)), ["old-solo", "new-solo"]);
        assert_eq!(ids(&apply_filters(&all, Filter::All, &racing, NOW)), ["new-duo", "new-solo"]);
        assert_eq!(ids(&apply_filters(&all, Filter::Solo, &racing, NOW)), ["new-solo"]);
        assert!(apply_filters(&all, Filter::All, &[String::from("nothing")], NOW).is_empty());
    }

    #[test]
    fn two_tags_in_the_address_keep_only_the_spaces_that_carry_both() {
        let all = [space("a", 1, 1.0, &["racing", "co-op"]), space("b", 1, 1.0, &["racing"]), space("c", 1, 1.0, &["co-op", "horror"]), space("d", 1, 1.0, &[])];
        let both = ["racing".to_string(), "co-op".to_string()];
        assert_eq!(ids(&apply_filters(&all, Filter::All, &both, NOW)), ["a"], "every tag, not any");
        assert_eq!(ids(&apply_filters(&all, Filter::All, &both[..1], NOW)), ["a", "b"]);
        assert!(apply_filters(&all, Filter::All, &["racing".to_string(), "horror".to_string()], NOW).is_empty());
    }

    #[test]
    fn a_space_with_no_tags_is_found_under_no_tag_and_a_genre_in_the_answer_is_not_one() {
        let l: Listing = serde_json::from_str("{\"id\":\"a\",\"genre\":\"Horror\",\"tags\":[]}").unwrap();
        assert!(apply_filters(&[l.clone()], Filter::All, &[String::from("horror")], NOW).is_empty(), "the genre is no tag");
        assert_eq!(ids(&apply_filters(&[l], Filter::All, &[], NOW)), ["a"], "it still shows with no tag asked for");
    }

    #[test]
    fn an_old_genre_link_filters_by_that_tag_and_the_address_is_rewritten_with_tag() {
        let picked = tags_in_address(&[], Some("Horror"));
        assert_eq!(picked, [String::from("horror")]);
        assert_eq!(gallery_href(&picked), "/gallery?tag=horror");
        let all = [space("a", 1, 1.0, &["horror"]), space("b", 1, 1.0, &["racing"])];
        assert_eq!(ids(&apply_filters(&all, Filter::All, &picked, NOW)), ["a"]);
        assert_eq!(gallery_href(&tags_in_address(&[String::from("co-op")], Some("Horror"))), "/gallery?tag=co-op&tag=horror");
    }

    #[test]
    fn the_chips_build_addresses_that_add_a_tag_or_take_it_off() {
        let on = vec![String::from("racing")];
        assert_eq!(gallery_href(&toggled(&on, "co-op")), "/gallery?tag=racing&tag=co-op");
        assert_eq!(gallery_href(&toggled(&on, "racing")), "/gallery");
        assert_eq!(gallery_href(&toggled(&[], "racing")), "/gallery?tag=racing");
    }

    #[test]
    fn the_headings_say_what_is_shown_and_a_tag_is_written_as_one() {
        let horror = [String::from("horror")];
        let two = [String::from("horror"), String::from("co-op")];
        assert_eq!(shelf_title(Filter::All, &[]), "All Spaces");
        assert_eq!(shelf_title(Filter::All, &horror), "#horror");
        assert_eq!(shelf_title(Filter::Solo, &horror), "#horror \u{00b7} Solo");
        assert_eq!(shelf_title(Filter::All, &two), "#horror #co-op");
        assert_eq!(nothing_here(Filter::All, &horror).0, "No Space uses #horror yet");
        assert_eq!(nothing_here(Filter::All, &two).0, "No Space uses #horror #co-op together yet");
        assert_eq!(nothing_here(Filter::New, &[]).0, "Nothing new in the last two weeks");
    }

    #[test]
    fn the_gallery_reads_the_word_genre_only_to_turn_an_old_link_into_a_tag() {
        let live = include_str!("gallery.rs").split("#[cfg(test)]").next().unwrap();
        assert_eq!(live.matches("get(\"genre\")").count(), 1, "one place reads it");
        assert_eq!(live.matches("legacy_genre_param(").count(), 3, "its definition and the two places that ask: the tags, and the rewrite of the address");
    }

    #[test]
    fn the_lead_label_claims_only_what_is_true() {
        let mut l = space("a", 1, 30.0, &[]);
        assert_eq!(lead_kicker(&l, NOW), "In the Gallery");
        l.published_at = Some(iso(2.0));
        assert_eq!(lead_kicker(&l, NOW), "New this week");
        l.featured = true;
        assert_eq!(lead_kicker(&l, NOW), "Featured");
    }

    #[test]
    fn a_live_session_is_found_by_its_space_and_the_corner_says_seats_or_room() {
        let live = LiveNow { sessions: vec![Session { id: "a".to_string(), players: 2, max_players: 4, ..Default::default() }], players_total: 2, sessions_total: 1 };
        let a = space("a", 10, 1.0, &[]);
        let b = space("b", 1, 1.0, &[]);
        assert_eq!(session_of(&live, "a").map(|s| s.players), Some(2));
        assert!(session_of(&live, "b").is_none());
        assert_eq!(corner_words(&a, session_of(&live, "a")), "2/4");
        assert_eq!(corner_words(&a, None), "up to 10");
        assert_eq!(corner_words(&b, None), "Solo");
    }

    #[test]
    fn the_lead_takes_turns_and_wraps_and_one_space_never_moves() {
        assert_eq!((0..5).scan(0, |i, _| { *i = next_lead(*i, 3); Some(*i) }).collect::<Vec<_>>(), [1, 2, 0, 1, 2]);
        assert_eq!(next_lead(0, 1), 0);
        assert_eq!(next_lead(0, 0), 0);
    }

    #[test]
    fn an_update_on_a_later_day_is_shown_beside_the_publish_day_and_the_same_day_is_not_repeated() {
        let published = Some("yesterday");
        assert_eq!(updated_label(Some(&iso(0.0)), published, NOW), Some("today".to_string()));
        assert_eq!(updated_label(Some(&iso(1.0)), published, NOW), None);
        assert_eq!(updated_label(None, published, NOW), None);
        assert_eq!(updated_label(Some("not a date"), published, NOW), None);
    }

    #[test]
    fn counts_read_the_way_a_person_says_them() {
        assert_eq!(players_words(1), "1 player");
        assert_eq!(players_words(0), "0 players");
        assert_eq!(players_words(7), "7 players");
        assert_eq!(sessions_words(1), "1 live session");
        assert_eq!(sessions_words(3), "3 live sessions");
        assert_eq!(seats_words(2, 4), "2/4");
        assert_eq!(seats_words(2, 0), "2");
    }
}
