// =============================================================================
// Eustress Web - The Community page
// =============================================================================
// The people building on Eustress and what is happening now, in the Gallery's look: glass panels, chips, mono numbers, and the
// Gallery's own cards (ListingCard, LiveCard). Every name, picture and number is something the Worker said, read when the page
// opens:
//
//   GET /api/community/stats          total_users, the Members figure. The route's other fields are fixed by the Worker, not
//                                     counted, so this page reads none of them.
//   GET /api/simulations/live         the live sessions and the players in them: Live now, and the Playing now figures.
//                                     Asked again every 20 seconds while the page is open.
//   GET /api/simulations              the newest listed Spaces (New in the Gallery), and each creator's newest Spaces
//   GET /api/makers?limit=8           the creators: people with a listed Space, most Spaces first, then most visits
//   GET /api/simulations/tags         the tags most used on listed Spaces, each a link to the Gallery's /gallery?tag= filter
//   GET /api/community/leaderboard    the Worker's ranking by hours in Eustress, all time; a row with no hours is left out
//   GET /api/community/search?q=      people search, by username
//
// Read and left out on purpose: the search answer's follower count (the Worker always sends 0 there), its generated
// suggestion text (not data about anyone), and the leaderboard's weekly pick, Space count and visit count (the count includes
// Spaces that are not in the Gallery). Not on the page because no route serves it: the people a signed-in visitor follows,
// which needs a list of the accounts the viewer follows (for example GET /api/social/following).
//
// Wherever a display name is shown, the @username is beside it (pages/profile.rs handle_of), and a picture is shown only from a
// path under /assets (pages/profile.rs safe_avatar); anything else draws the first letter. A section with nothing to show says
// so in its own words, and a section whose route failed says that instead, never "nothing".
//
// Table of Contents:
// 1. What the Worker says, and the pure rules (tested)
// 2. The page
// 3. The sections
// 4. People search
// 5. Small parts: avatar, empty and failed states, loading shapes
// 6. Tests
// =============================================================================

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;

use crate::api::catalog::{self, Listing, LiveNow, Maker, TagCount};
use crate::api::{self, ApiClient};
use crate::components::{CentralNav, Footer};
use crate::pages::gallery::{players_words, sessions_words, ListingCard, LiveCard};
use crate::pages::profile::{handle_of, made_by, monogram, safe_avatar, spaces_words};
use crate::state::AppState;
use crate::tags::{gallery_href, normalize_tag};
use crate::utils::format_count;

/// How often the live sessions are asked again while the page is open.
const LIVE_REFRESH_MS: u32 = 20_000;
/// How many of the newest Spaces are shown.
const NEWEST: usize = 6;
/// How many creators are asked for.
const CREATORS: usize = 8;
/// How many of a creator's newest Spaces are named on their row.
const CREATOR_SPACES: usize = 2;
/// How many tags are offered.
const TAGS: usize = 16;
/// How many rows of the hours ranking are shown.
const BOARD_ROWS: usize = 10;
/// How many people a search shows.
const SEARCH_LIMIT: usize = 10;
/// The longest search text sent; a longer one matches no username.
const SEARCH_MAX: usize = 64;

// -----------------------------------------------------------------------------
// 1. What the Worker says, and the pure rules
// -----------------------------------------------------------------------------

/// One section's data: still loading, here, or its route failed.
#[derive(Clone, Debug, PartialEq)]
pub enum Load<T> {
    Loading,
    Ready(T),
    Failed,
}

/// `GET /api/community/stats`: the one field this page reads.
#[derive(Debug, Default, Deserialize)]
struct StatsAnswer {
    #[serde(default)]
    total_users: u64,
}

#[derive(Debug, Default, Deserialize)]
struct MakersAnswer {
    #[serde(default)]
    makers: Vec<Maker>,
}

#[derive(Debug, Default, Deserialize)]
struct TagsAnswer {
    #[serde(default)]
    tags: Vec<TagCount>,
}

/// One row of `GET /api/community/leaderboard`, the fields this page reads.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct BoardEntry {
    #[serde(default)]
    pub rank: u32,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
    /// All-time hours, to a tenth.
    #[serde(default)]
    pub hours: f64,
}

#[derive(Debug, Default, Deserialize)]
struct BoardAnswer {
    #[serde(default)]
    entries: Vec<BoardEntry>,
}

/// One person in `GET /api/community/search`, the fields this page reads.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Person {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    /// True only when the account's identity was verified.
    #[serde(default)]
    pub is_verified: bool,
}

#[derive(Debug, Default, Deserialize)]
struct SearchAnswer {
    #[serde(default)]
    users: Vec<Person>,
}

/// The name a person's row leads with: the display name, or the username when there is none.
pub fn person_name(person: &Person) -> String {
    person.display_name.as_deref().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&person.username).to_string()
}

/// `/profile/ada`.
pub fn profile_href(username: &str) -> String {
    format!("/profile/{}", urlencoding::encode(username))
}

/// The text a search sends: trimmed, one leading @ dropped, at least two characters, at most 64. `None` means nothing is asked.
pub fn search_ready(raw: &str) -> Option<String> {
    let text = raw.trim();
    let text = text.strip_prefix('@').unwrap_or(text).trim();
    (text.chars().count() >= 2).then(|| text.chars().take(SEARCH_MAX).collect())
}

/// The ranking as the Worker ordered it, without the rows that have no hours or no name, at most `n`.
pub fn ranked_hours(entries: &[BoardEntry], n: usize) -> Vec<BoardEntry> {
    entries.iter().filter(|e| e.hours > 0.0 && !e.username.trim().is_empty()).take(n).cloned().collect()
}

/// "312 h", "4.5 h".
pub fn hours_words(hours: f64) -> String {
    if hours >= 100.0 { format!("{} h", format_count(hours.round() as u64)) } else { format!("{:.1} h", hours) }
}

/// The tags to offer, each with its Gallery link and how many Spaces use it. A tag the Worker would not accept is left out.
pub fn tag_links(tags: &[TagCount], n: usize) -> Vec<(String, String, u32)> {
    let mut out: Vec<(String, String, u32)> = Vec::new();
    for t in tags {
        let Some(tag) = normalize_tag(&t.tag) else { continue };
        if out.iter().any(|(seen, _, _)| *seen == tag) {
            continue;
        }
        let href = gallery_href(std::slice::from_ref(&tag));
        out.push((tag, href, t.count));
        if out.len() == n {
            break;
        }
    }
    out
}

/// "3 Spaces · 1,204 visits".
pub fn maker_line(maker: &Maker) -> String {
    let visits = if maker.visits == 1 { "1 visit".to_string() } else { format!("{} visits", format_count(maker.visits)) };
    format!("{} \u{00b7} {visits}", spaces_words(maker.space_count as usize))
}

/// A creator's newest listed Spaces, in the Worker's order (newest first), at most `n`.
pub fn newest_by(listings: &[Listing], username: &str, n: usize) -> Vec<Listing> {
    made_by(listings, username).into_iter().take(n).collect()
}

/// The figures in the header, only those that are known: Members once the count is in (the Worker answers 0 when it could not
/// count, so 0 is left out), and the live figures once the live list answered.
pub fn figures_of(members: Option<u64>, live: &Load<LiveNow>) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if let Some(n) = members.filter(|n| *n > 0) {
        out.push(("Members", format_count(n)));
    }
    if let Load::Ready(now) = live {
        out.push(("Playing now", format_count(now.players_total as u64)));
        out.push(("Live sessions", format_count(now.sessions_total.max(now.sessions.len() as u32) as u64)));
    }
    out
}

// -----------------------------------------------------------------------------
// 2. The page
// -----------------------------------------------------------------------------

/// What every section reads. The page fills it from the Worker; a test fills it by hand.
#[derive(Clone, Copy)]
pub struct Feeds {
    pub members: RwSignal<Option<u64>>,
    pub live: RwSignal<Load<LiveNow>>,
    pub listings: RwSignal<Load<Vec<Listing>>>,
    pub makers: RwSignal<Load<Vec<Maker>>>,
    pub tags: RwSignal<Load<Vec<TagCount>>>,
    pub board: RwSignal<Load<Vec<BoardEntry>>>,
}

impl Feeds {
    pub fn loading() -> Self {
        Self {
            members: RwSignal::new(None),
            live: RwSignal::new(Load::Loading),
            listings: RwSignal::new(Load::Loading),
            makers: RwSignal::new(Load::Loading),
            tags: RwSignal::new(Load::Loading),
            board: RwSignal::new(Load::Loading),
        }
    }
}

fn ready_or_failed<T>(answer: Result<T, api::ApiError>) -> Load<T> {
    match answer {
        Ok(value) => Load::Ready(value),
        Err(_) => Load::Failed,
    }
}

#[component]
pub fn CommunityPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let feeds = Feeds::loading();
    let runs = StoredValue::new(0u64);

    // In the browser only: each route once, side by side, and the live sessions again every little while.
    Effect::new(move |_| {
        spawn_local(async move {
            let client = ApiClient::new(api_url.get_value());
            let stats = client.get::<StatsAnswer>("/api/community/stats").await;
            feeds.members.set(stats.ok().map(|s| s.total_users));
        });
        spawn_local(async move {
            let client = ApiClient::new(api_url.get_value());
            feeds.listings.set(ready_or_failed(catalog::fetch_listings(&client).await));
        });
        spawn_local(async move {
            let client = ApiClient::new(api_url.get_value());
            let path = format!("/api/makers?limit={CREATORS}");
            feeds.makers.set(ready_or_failed(client.get::<MakersAnswer>(&path).await.map(|a| a.makers)));
        });
        spawn_local(async move {
            let client = ApiClient::new(api_url.get_value());
            feeds.tags.set(ready_or_failed(client.get::<TagsAnswer>("/api/simulations/tags").await.map(|a| a.tags)));
        });
        spawn_local(async move {
            let client = ApiClient::new(api_url.get_value());
            feeds.board.set(ready_or_failed(client.get::<BoardAnswer>("/api/community/leaderboard").await.map(|a| a.entries)));
        });
        let run = runs.get_value() + 1;
        runs.set_value(run);
        spawn_local(async move {
            loop {
                let client = ApiClient::new(api_url.get_value());
                let answer = client.get::<LiveNow>("/api/simulations/live").await;
                if runs.try_get_value() != Some(run) {
                    return;
                }
                // A refresh that fails keeps what the last good answer showed.
                match answer {
                    Ok(now) => feeds.live.set(Load::Ready(now)),
                    Err(_) if feeds.live.get_untracked() == Load::Loading => feeds.live.set(Load::Failed),
                    Err(_) => {}
                }
                TimeoutFuture::new(LIVE_REFRESH_MS).await;
                if runs.try_get_value() != Some(run) {
                    return;
                }
            }
        });
    });

    view! {
        <div class="page g-page cm-page">
            <CentralNav active="community".to_string() />
            <div class="g-backdrop" aria-hidden="true"></div>
            <main class="g-main">
                <CommunityBody feeds=feeds />
            </main>
            <Footer />
        </div>
    }
}

/// Everything under the top bar.
#[component]
pub fn CommunityBody(feeds: Feeds) -> impl IntoView {
    // The Gallery's cards read the live sessions to mark a Space that is live.
    let cards_live = RwSignal::new(match feeds.live.get_untracked() {
        Load::Ready(now) => now,
        _ => LiveNow::default(),
    });
    Effect::new(move |_| {
        if let Load::Ready(now) = feeds.live.get() {
            cards_live.set(now);
        }
    });

    view! {
        <div class="g-wrap cm-wrap">
            <header class="g-glass cm-hero" aria-labelledby="cm-title">
                <div class="cm-hero-copy">
                    <div class="g-kicker"><span class="g-kick-rule"></span>"Community"</div>
                    <h1 id="cm-title" class="cm-title">"The people building on Eustress"</h1>
                    <p class="cm-lede">"Who is hosting right now, who is publishing Spaces, and what they made. Find anyone by their username."</p>
                    <PeopleSearch />
                </div>
                {move || {
                    let figures = figures_of(feeds.members.get(), &feeds.live.get());
                    (!figures.is_empty()).then(|| view! {
                        <dl class="cm-figures">
                            {figures.into_iter().map(|(k, v)| view! { <div><dt>{k}</dt><dd>{v}</dd></div> }).collect_view()}
                        </dl>
                    })
                }}
            </header>

            <div class="cm-layout">
                <div class="cm-main">
                    <LiveSection live=feeds.live />
                    <NewestSection listings=feeds.listings live=cards_live />
                </div>
                <aside class="cm-side" aria-label="Creators, tags and hours">
                    <CreatorsSection makers=feeds.makers listings=feeds.listings />
                    <TagsSection tags=feeds.tags />
                    <HoursSection board=feeds.board />
                </aside>
            </div>

            <div class="g-glass g-band cm-band">
                <div>
                    <h2>"Publish a Space and you are part of this page"</h2>
                    <p>"Build it in Eustress Studio and publish it. Once it is approved you are listed under Creators, and while someone hosts it, it shows under Live now."</p>
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
// 3. The sections
// -----------------------------------------------------------------------------

#[component]
fn LiveSection(live: RwSignal<Load<LiveNow>>) -> impl IntoView {
    view! {
        <section class="g-section cm-section" aria-labelledby="cm-live-h">
            <div class="g-head"><h2 id="cm-live-h">"Live now"</h2><span class="g-rule"></span></div>
            {move || match live.get() {
                Load::Loading => view! { <div class="g-rail" role="status" aria-busy="true" aria-label="Loading the live sessions">{(0..2).map(|_| view! { <div class="g-skel cm-skel-live"></div> }).collect_view()}</div> }.into_any(),
                Load::Failed => failed_state("The live list is not available right now", "Live sessions show here again as soon as it answers.").into_any(),
                Load::Ready(now) if now.sessions.is_empty() => {
                    empty_state("Nobody is hosting right now", "Live sessions appear here the moment a host goes live.", None).into_any()
                }
                Load::Ready(now) => {
                    let total = now.sessions_total.max(now.sessions.len() as u32);
                    view! {
                        <p class="cm-note"><span class="g-dot" aria-hidden="true"></span>{format!("{} in {}", players_words(now.players_total), sessions_words(total))}</p>
                        <div class="g-rail">{now.sessions.into_iter().map(|s| view! { <LiveCard session=s /> }).collect_view()}</div>
                    }.into_any()
                }
            }}
        </section>
    }
}

#[component]
fn NewestSection(listings: RwSignal<Load<Vec<Listing>>>, live: RwSignal<LiveNow>) -> impl IntoView {
    view! {
        <section class="g-section cm-section" aria-labelledby="cm-new-h">
            <div class="g-head"><h2 id="cm-new-h">"New in the Gallery"</h2><span class="g-rule"></span><a class="cm-more" href="/gallery">"Open the Gallery"</a></div>
            {move || match listings.get() {
                Load::Loading => view! {
                    <div class="g-grid cm-grid" role="status" aria-busy="true" aria-label="Loading the newest Spaces">
                        {(0..3).map(|_| view! { <div class="g-card g-skel-card"><div class="g-skel g-skel-media"></div><div class="g-body"><span class="g-skel w70"></span><span class="g-skel w40"></span></div></div> }).collect_view()}
                    </div>
                }.into_any(),
                Load::Failed => failed_state("The Gallery could not load right now", "The newest Spaces show here again as soon as it answers.").into_any(),
                Load::Ready(list) if list.is_empty() => {
                    empty_state("Nothing is published yet", "Spaces appear here once they are approved.", Some(("/download", "Download Studio"))).into_any()
                }
                Load::Ready(list) => view! {
                    <div class="g-grid cm-grid">
                        {list.into_iter().take(NEWEST).map(|l| view! { <ListingCard listing=l live=live /> }).collect_view()}
                    </div>
                }.into_any(),
            }}
        </section>
    }
}

#[component]
fn CreatorsSection(makers: RwSignal<Load<Vec<Maker>>>, listings: RwSignal<Load<Vec<Listing>>>) -> impl IntoView {
    view! {
        <section class="g-section cm-section" aria-labelledby="cm-makers-h">
            <div class="g-head"><h2 id="cm-makers-h">"Creators"</h2><span class="g-rule"></span></div>
            {move || match makers.get() {
                Load::Loading => loading_rows("Loading the creators").into_any(),
                Load::Failed => failed_state("The creators list is not available right now", "It shows here again as soon as it answers.").into_any(),
                Load::Ready(list) if list.is_empty() => {
                    empty_state("No creators in the Gallery yet", "A creator is listed here once one of their Spaces is approved.", Some(("/download", "Download Studio"))).into_any()
                }
                Load::Ready(list) => {
                    let all = match listings.get() {
                        Load::Ready(all) => Some(all),
                        _ => None,
                    };
                    view! {
                        <ol class="g-glass cm-list">
                            {list.into_iter().map(|m| {
                                let handle = handle_of(&m.username);
                                let href = profile_href(&m.username);
                                let line = maker_line(&m);
                                let newest = all.as_deref().map(|all| newest_by(all, &m.username, CREATOR_SPACES)).unwrap_or_default();
                                view! {
                                    <li class="cm-row">
                                        <Avatar name=m.username.clone() url=m.avatar_url.clone() />
                                        <div class="cm-who">
                                            <a class="cm-handle cm-strong" href=href>{handle}</a>
                                            <span class="cm-sub">{line}</span>
                                            {(!newest.is_empty()).then(|| view! {
                                                <span class="cm-spaces">
                                                    "Newest: "
                                                    {newest.into_iter().enumerate().map(|(i, l)| {
                                                        let name = if l.name.trim().is_empty() { "Untitled Space".to_string() } else { l.name.clone() };
                                                        let href = format!("/simulation/{}", urlencoding::encode(&l.id));
                                                        view! { {(i > 0).then_some(", ")}<a href=href>{name}</a> }
                                                    }).collect_view()}
                                                </span>
                                            })}
                                        </div>
                                    </li>
                                }
                            }).collect_view()}
                        </ol>
                    }.into_any()
                }
            }}
        </section>
    }
}

#[component]
fn TagsSection(tags: RwSignal<Load<Vec<TagCount>>>) -> impl IntoView {
    view! {
        <section class="g-section cm-section" aria-labelledby="cm-tags-h">
            <div class="g-head"><h2 id="cm-tags-h">"Popular tags"</h2><span class="g-rule"></span></div>
            {move || match tags.get() {
                Load::Loading => view! { <div class="g-chips" role="status" aria-busy="true" aria-label="Loading the tags">{(0..6).map(|_| view! { <span class="g-skel cm-skel-chip"></span> }).collect_view()}</div> }.into_any(),
                Load::Failed => failed_state("The tags are not available right now", "They show here again as soon as the Gallery answers.").into_any(),
                Load::Ready(list) => {
                    let links = tag_links(&list, TAGS);
                    if links.is_empty() {
                        empty_state("No tags yet", "Tags appear here as creators add them to their Spaces.", None).into_any()
                    } else {
                        view! {
                            <div class="g-chips cm-tags">
                                {links.into_iter().map(|(tag, href, count)| {
                                    let label = if count == 1 { format!("{tag}, 1 Space") } else { format!("{tag}, {count} Spaces") };
                                    view! { <a class="g-chip g-tag" href=href aria-label=label>{tag}<span class="cm-count">{count}</span></a> }
                                }).collect_view()}
                            </div>
                        }.into_any()
                    }
                }
            }}
        </section>
    }
}

#[component]
fn HoursSection(board: RwSignal<Load<Vec<BoardEntry>>>) -> impl IntoView {
    view! {
        <section class="g-section cm-section" aria-labelledby="cm-hours-h">
            <div class="g-head"><h2 id="cm-hours-h">"Most hours in Eustress"</h2><span class="g-rule"></span><a class="cm-more" href="/leaderboard">"Full list"</a></div>
            {move || match board.get() {
                Load::Loading => loading_rows("Loading the hours").into_any(),
                Load::Failed => failed_state("The hours are not available right now", "They show here again as soon as the leaderboard answers.").into_any(),
                Load::Ready(entries) => {
                    let rows = ranked_hours(&entries, BOARD_ROWS);
                    if rows.is_empty() {
                        empty_state("No hours counted yet", "Hours appear here once a day of signed-in time in Eustress has been counted.", None).into_any()
                    } else {
                        view! {
                            <ol class="g-glass cm-list cm-board">
                                {rows.into_iter().map(|e| {
                                    let handle = handle_of(&e.username);
                                    let href = profile_href(&e.username);
                                    view! {
                                        <li class="cm-row">
                                            <span class="cm-rank">{e.rank}</span>
                                            <Avatar name=e.username.clone() url=e.avatar_url.clone() />
                                            <a class="cm-handle cm-strong cm-grow" href=href>{handle}</a>
                                            <span class="cm-hours">{hours_words(e.hours)}</span>
                                        </li>
                                    }
                                }).collect_view()}
                            </ol>
                            <p class="cm-foot">"All time. Hours Eustress ran while signed in, added when each day ends."</p>
                        }.into_any()
                    }
                }
            }}
        </section>
    }
}

// -----------------------------------------------------------------------------
// 4. People search
// -----------------------------------------------------------------------------

/// Find people by username: the Worker's community search, asked when the form is sent.
#[component]
fn PeopleSearch() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let text = RwSignal::new(String::new());
    let asked = RwSignal::new(Option::<(String, Load<Vec<Person>>)>::None);
    let too_short = RwSignal::new(false);
    let seq = StoredValue::new(0u64);

    let run = move || {
        let n = seq.get_value() + 1;
        seq.set_value(n);
        let Some(q) = search_ready(&text.get_untracked()) else {
            asked.set(None);
            too_short.set(!text.get_untracked().trim().is_empty());
            return;
        };
        too_short.set(false);
        asked.set(Some((q.clone(), Load::Loading)));
        spawn_local(async move {
            let client = ApiClient::new(api_url.get_value());
            let path = format!("/api/community/search?q={}&limit={SEARCH_LIMIT}", urlencoding::encode(&q));
            let answer = client.get::<SearchAnswer>(&path).await;
            if seq.try_get_value() != Some(n) {
                return;
            }
            asked.set(Some((q, ready_or_failed(answer.map(|a| a.users)))));
        });
    };

    view! {
        <form class="cm-search" role="search" on:submit=move |ev: leptos::ev::SubmitEvent| { ev.prevent_default(); run(); }>
            <label class="g-sr" for="cm-q">"Find people by username"</label>
            <svg class="cm-search-icon" viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="M20 20l-3.6-3.6"/></svg>
            <input id="cm-q" class="cm-q" type="search" placeholder="Find people by username" autocomplete="off" spellcheck="false" maxlength="64"
                prop:value=move || text.get()
                on:input=move |e| {
                    let value = event_target_value(&e);
                    if value.trim().is_empty() {
                        asked.set(None);
                        too_short.set(false);
                    }
                    text.set(value);
                } />
            <button type="submit" class="g-btn g-btn-primary">"Search"</button>
        </form>
        {move || too_short.get().then(|| view! { <p class="cm-hint" role="status">"Type at least two letters of a username."</p> })}
        {move || asked.get().map(|(query, found)| view! { <PeopleResults query=query found=found /> })}
    }
}

/// What a search found: each person with their display name and @username, or what went wrong.
#[component]
pub fn PeopleResults(query: String, found: Load<Vec<Person>>) -> impl IntoView {
    let body = match found {
        Load::Loading => view! { <p class="cm-hint" role="status" aria-busy="true">"Searching"</p> }.into_any(),
        Load::Failed => view! { <p class="cm-hint" role="status">"Search is not available right now. Try again in a minute."</p> }.into_any(),
        Load::Ready(people) if people.is_empty() => view! {
            <div class="cm-empty-inline">
                <b>{format!("No username matches \u{201c}{query}\u{201d}")}</b>
                <p>"Check the spelling. Search looks at usernames, not display names."</p>
            </div>
        }.into_any(),
        Load::Ready(people) => view! {
            <ul class="cm-people">
                {people.into_iter().filter(|p| !p.username.trim().is_empty()).map(|p| {
                    let name = person_name(&p);
                    let handle = handle_of(&p.username);
                    let href = profile_href(&p.username);
                    view! {
                        <li>
                            <a class="cm-person" href=href>
                                <Avatar name=name.clone() url=p.avatar_url.clone() />
                                <span class="cm-who">
                                    <span class="cm-name-row"><span class="cm-name cm-strong">{name}</span><span class="cm-handle">{handle}</span></span>
                                    {p.is_verified.then(|| view! { <span class="g-chip pn-verified cm-verified">"Verified"</span> })}
                                </span>
                                <span class="cm-go" aria-hidden="true">"\u{2192}"</span>
                            </a>
                        </li>
                    }
                }).collect_view()}
            </ul>
        }.into_any(),
    };
    view! {
        <section class="cm-results" aria-label="People found">
            {body}
        </section>
    }
}

// -----------------------------------------------------------------------------
// 5. Small parts
// -----------------------------------------------------------------------------

/// A person's picture from a path under /assets, or the first letter of their name.
#[component]
fn Avatar(name: String, url: Option<String>) -> impl IntoView {
    let picture = safe_avatar(url.as_deref());
    let letter = monogram(&name);
    view! {
        <span class="cm-av" aria-hidden="true">
            {match picture {
                Some(src) => view! { <img src=src alt="" loading="lazy" decoding="async" /> }.into_any(),
                None => view! { <span class="cm-letter">{letter}</span> }.into_any(),
            }}
        </span>
    }
}

const EMPTY_GLYPH: &str = "M12 3a9 9 0 1 0 0 18a9 9 0 0 0 0-18zM8 12h8";
const FAILED_GLYPH: &str = "M12 3a9 9 0 1 0 0 18a9 9 0 0 0 0-18zM12 7.5v5.5M12 16.5v.01";

/// A section with nothing to show: what is missing, and how it fills.
fn empty_state(title: &'static str, hint: &'static str, action: Option<(&'static str, &'static str)>) -> impl IntoView {
    view! {
        <div class="g-glass cm-empty">
            <span class="g-glyph" aria-hidden="true"><svg viewBox="0 0 24 24"><path d=EMPTY_GLYPH/></svg></span>
            <div>
                <b>{title}</b>
                <p>{hint}</p>
                {action.map(|(href, label)| view! { <a class="g-btn g-btn-sm" href=href>{label}</a> })}
            </div>
        </div>
    }
}

/// A section whose route did not answer: said plainly, never shown as empty.
fn failed_state(title: &'static str, hint: &'static str) -> impl IntoView {
    view! {
        <div class="g-glass cm-empty cm-failed" role="status">
            <span class="g-glyph" aria-hidden="true"><svg viewBox="0 0 24 24"><path d=FAILED_GLYPH/></svg></span>
            <div>
                <b>{title}</b>
                <p>{hint}</p>
            </div>
        </div>
    }
}

/// The shape of a list while it loads, not made-up rows.
fn loading_rows(label: &'static str) -> impl IntoView {
    view! {
        <div class="g-glass cm-list" role="status" aria-busy="true" aria-label=label>
            {(0..4).map(|_| view! { <div class="cm-row"><span class="cm-av g-skel"></span><span class="g-skel w60"></span></div> }).collect_view()}
        </div>
    }
}

// -----------------------------------------------------------------------------
// 6. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::catalog::Session;

    const FILE: &str = include_str!("community.rs");
    const CSS: &str = include_str!("../../style/community.css");
    const MAIN_CSS: &str = include_str!("../../style/main.css");
    const INDEX: &str = include_str!("../../index.html");

    /// The page's code, without these tests.
    fn source() -> &'static str {
        FILE.split(concat!("#[cfg", "(test)]")).next().unwrap()
    }

    fn space(id: &str, author: &str) -> Listing {
        Listing { id: id.to_string(), name: format!("Space {id}"), author_name: Some(author.to_string()), max_players: 4, ..Default::default() }
    }

    fn maker(name: &str, avatar: Option<&str>) -> Maker {
        Maker { username: name.to_string(), avatar_url: avatar.map(str::to_string), space_count: 2, visits: 1204 }
    }

    fn row(rank: u32, name: &str, hours: f64) -> BoardEntry {
        BoardEntry { rank, username: name.to_string(), avatar_url: None, hours }
    }

    fn live_one() -> LiveNow {
        LiveNow {
            sessions: vec![Session { id: "a".to_string(), name: "Space a".to_string(), author_name: Some("ada".to_string()), players: 2, max_players: 8, ..Default::default() }],
            players_total: 2,
            sessions_total: 1,
        }
    }

    struct Data {
        members: Option<u64>,
        live: Load<LiveNow>,
        listings: Load<Vec<Listing>>,
        makers: Load<Vec<Maker>>,
        tags: Load<Vec<TagCount>>,
        board: Load<Vec<BoardEntry>>,
    }

    fn full() -> Data {
        Data {
            members: Some(1534),
            live: Load::Ready(live_one()),
            listings: Load::Ready(vec![space("a", "ada"), space("b", "bob"), space("c", "ada"), space("d", "ada")]),
            makers: Load::Ready(vec![maker("ada", Some("https://evil.example/a.png")), maker("bob", Some("/assets/avatars/bob.png"))]),
            tags: Load::Ready(vec![TagCount { tag: "horror".to_string(), count: 5 }, TagCount { tag: "co-op".to_string(), count: 1 }]),
            board: Load::Ready(vec![row(1, "ada", 312.4), row(2, "bob", 4.5), row(3, "cyd", 0.0)]),
        }
    }

    fn all(state: fn() -> Load<()>) -> Data {
        fn as_<T>(l: Load<()>) -> Load<T> {
            match l {
                Load::Loading => Load::Loading,
                Load::Failed => Load::Failed,
                Load::Ready(()) => unreachable!(),
            }
        }
        Data { members: None, live: as_(state()), listings: as_(state()), makers: as_(state()), tags: as_(state()), board: as_(state()) }
    }

    fn empty() -> Data {
        Data { members: Some(0), live: Load::Ready(LiveNow::default()), listings: Load::Ready(vec![]), makers: Load::Ready(vec![]), tags: Load::Ready(vec![]), board: Load::Ready(vec![]) }
    }

    /// The page body, rendered on the server the way the prerender renders a page.
    fn render(data: Data) -> String {
        use leptos::tachys::view::RenderHtml;
        let owner = Owner::new();
        owner.with(|| {
            provide_context(AppState::new());
            crate::components::play_chooser::provide_play_chooser();
            let feeds = Feeds {
                members: RwSignal::new(data.members),
                live: RwSignal::new(data.live),
                listings: RwSignal::new(data.listings),
                makers: RwSignal::new(data.makers),
                tags: RwSignal::new(data.tags),
                board: RwSignal::new(data.board),
            };
            view! { <CommunityBody feeds=feeds /> }.to_html()
        })
    }

    fn render_people(query: &str, found: Load<Vec<Person>>) -> String {
        use leptos::tachys::view::RenderHtml;
        let owner = Owner::new();
        owner.with(|| view! { <PeopleResults query=query.to_string() found=found /> }.to_html())
    }

    fn person(username: &str, display: Option<&str>, avatar: Option<&str>) -> Person {
        Person { username: username.to_string(), display_name: display.map(str::to_string), avatar_url: avatar.map(str::to_string), is_verified: false }
    }

    #[cfg(feature = "ssr")]
    fn render_app_at(url: &str) -> String {
        use hydration_context::{SharedContext, SsrSharedContext};
        use leptos::tachys::view::RenderHtml;
        use leptos_router::location::RequestUrl;
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

    /// What the old page showed that no route said, and the routes' fields it read that the Worker does not count.
    const STAND_INS: [&str; 16] = [
        concat!("Racing games are ", "up 45%"),
        concat!("Build ", "Jam"),
        concat!("/events/", "build-jam"),
        concat!("/re", "wards"),
        concat!("Creator ", "Rewards"),
        concat!("Join thou", "sands"),
        concat!("Work Hard. ", "Play Hard."),
        concat!("amazing ", "worlds"),
        concat!("FEATURED ", "CREATOR"),
        concat!("Trending This ", "Week"),
        concat!("category=", "racing"),
        concat!("ai_", "suggestion"),
        concat!("online_", "now"),
        concat!("total_", "simulations"),
        concat!("total_", "plays"),
        concat!("page-community-", "industrial"),
    ];

    #[test]
    fn nothing_the_old_page_invented_comes_back() {
        for gone in STAND_INS {
            assert!(!source().contains(gone), "community.rs reads or shows {gone:?}");
        }
        for (what, html) in [("full", render(full())), ("empty", render(empty())), ("failed", render(all(|| Load::Failed)))] {
            for gone in STAND_INS {
                assert!(!html.contains(gone), "the {what} page shows {gone:?}");
            }
            for medal in ['\u{1f3c6}', '\u{1f948}', '\u{1f949}'] {
                assert!(!html.contains(medal), "the {what} page shows a medal");
            }
            assert!(!html.contains("Loading..."), "no stand-in name");
        }
        assert!(!source().contains(concat!("feat", "ured")), "the leaderboard's weekly pick is not read");
    }

    #[test]
    fn every_section_has_its_own_empty_state() {
        let html = render(empty());
        for said in [
            "Nobody is hosting right now",
            "Nothing is published yet",
            "No creators in the Gallery yet",
            "No tags yet",
            "No hours counted yet",
        ] {
            assert!(html.contains(said), "the empty page says {said:?}");
        }
        assert_eq!(html.matches("class=\"g-glass cm-empty\"").count(), 5, "one designed empty state per section");
        for drawn in ["class=\"g-card", "g-live-card", "class=\"cm-row\"", "class=\"g-chip g-tag\"", "<dt>Members</dt>"] {
            assert!(!html.contains(drawn), "nothing is drawn for nothing: {drawn}");
        }
        assert!(html.contains("<dt>Playing now</dt><dd>0</dd>"), "nobody playing is a real answer, and it is shown");
        let nobody = render_people("zz", Load::Ready(vec![]));
        assert!(nobody.contains("No username matches \u{201c}zz\u{201d}"), "{nobody}");
    }

    #[test]
    fn a_section_whose_route_failed_says_so_and_never_claims_it_is_empty() {
        let html = render(all(|| Load::Failed));
        for said in [
            "The live list is not available right now",
            "The Gallery could not load right now",
            "The creators list is not available right now",
            "The tags are not available right now",
            "The hours are not available right now",
        ] {
            assert!(html.contains(said), "the failed page says {said:?}");
        }
        for empty in ["Nobody is hosting", "Nothing is published", "No creators", "No tags yet", "No hours counted"] {
            assert!(!html.contains(empty), "a failed route is not an empty one: {empty}");
        }
        let loading = render(all(|| Load::Loading));
        for empty in ["Nobody is hosting", "Nothing is published", "No creators", "No tags yet", "No hours counted", "not available"] {
            assert!(!loading.contains(empty), "a section still loading claims nothing: {empty}");
        }
        assert!(loading.contains("aria-busy=\"true\""));
        assert!(render_people("ada", Load::Failed).contains("Search is not available right now"));
    }

    #[test]
    fn the_username_is_beside_every_display_name() {
        let html = render_people("ad", Load::Ready(vec![person("ada", Some("Ada Lovelace"), None), person("adam", None, None), person("", Some("No Name"), None)]));
        assert_eq!(html.matches("class=\"cm-name cm-strong\"").count(), 2, "a row without a username is not drawn");
        assert_eq!(html.matches("class=\"cm-handle\"").count(), html.matches("class=\"cm-name cm-strong\"").count(), "every name has its handle");
        for (name, handle) in [("Ada Lovelace", "@ada"), (">adam<", "@adam")] {
            let at = html.find(name).unwrap_or_else(|| panic!("{name} is shown"));
            let row_end = html[at..].find("</a>").map(|i| at + i).unwrap();
            assert!(html[at..row_end].contains(handle), "{handle} is beside {name}");
        }
        assert!(!html.contains("No Name"));
        // Creators and the hours list name people by @username itself.
        let page = render(full());
        for handle in [">@ada<", ">@bob<"] {
            assert!(page.contains(handle), "{handle}");
        }
        assert!(!page.contains(">ada<") && !page.contains(">bob<"), "no bare username stands in for a name");
        assert_eq!(person_name(&person("ada", Some("  "), None)), "ada");
    }

    #[test]
    fn a_picture_comes_only_from_a_path_under_assets() {
        let page = render(full());
        assert!(!page.contains("evil.example"), "an address on another host is never drawn");
        assert!(page.contains("<img src=\"/assets/avatars/bob.png\""), "a path under /assets is");
        // Every avatar on the page is a picture under /assets or a letter.
        for chunk in page.split("class=\"cm-av\"").skip(1) {
            let inner = &chunk[..chunk.find("</span>").unwrap()];
            assert!(inner.contains("cm-letter") || inner.contains("src=\"/assets/"), "{inner}");
        }
        let people = render_people("ad", Load::Ready(vec![person("ada", None, Some("https://cdn.eustress.dev/a.png")), person("adb", None, Some("javascript:alert(1)")), person("adc", None, Some("/assets/a.png"))]));
        assert!(!people.contains("cdn.eustress.dev") && !people.contains("javascript:"));
        assert_eq!(people.matches("<img ").count(), 1);
        assert!(people.contains("cm-letter\">A<"));
    }

    #[test]
    fn the_tag_chips_link_to_the_gallerys_tag_filter() {
        let tags = vec![
            TagCount { tag: "horror".to_string(), count: 5 },
            TagCount { tag: "Co-Op".to_string(), count: 1 },
            TagCount { tag: "<b>x</b>".to_string(), count: 9 },
            TagCount { tag: "horror".to_string(), count: 2 },
        ];
        let links = tag_links(&tags, 16);
        assert_eq!(links, vec![("horror".to_string(), "/gallery?tag=horror".to_string(), 5), ("co-op".to_string(), "/gallery?tag=co-op".to_string(), 1)]);
        assert_eq!(tag_links(&tags, 1).len(), 1);
        let html = render(full());
        assert!(html.contains("href=\"/gallery?tag=horror\""));
        assert!(html.contains("href=\"/gallery?tag=co-op\""));
        let cloud = &html[html.find("cm-tags").unwrap()..];
        let cloud = &cloud[..cloud.find("</section>").unwrap()];
        assert_eq!(cloud.matches("<a ").count(), 2);
        assert_eq!(cloud.matches("href=\"/gallery?tag=").count(), 2, "every chip is a Gallery tag filter");
        assert!(!html.contains("category="));
    }

    #[test]
    fn the_copy_has_no_dashes_and_names_the_product_plainly() {
        let pages = [
            render(full()),
            render(empty()),
            render(all(|| Load::Failed)),
            render(all(|| Load::Loading)),
            render_people("ad", Load::Ready(vec![person("ada", Some("Ada"), None)])),
            render_people("zz", Load::Ready(vec![])),
            render_people("zz", Load::Failed),
        ];
        for dash in ['\u{2014}', '\u{2013}'] {
            assert!(!source().contains(dash), "community.rs");
            assert!(!CSS.contains(dash), "community.css");
            for html in &pages {
                assert!(!html.contains(dash), "the rendered page");
            }
        }
        for html in pages.iter().map(|h| h.to_lowercase()).chain(std::iter::once(source().to_lowercase())) {
            assert!(!html.contains("game engine"), "Eustress is not called a game engine");
            assert!(!html.contains("experience"), "the product noun is Simulation or Space");
        }
    }

    #[test]
    fn community_css_is_linked_once_and_before_topbar_css() {
        let link = "<link data-trunk rel=\"css\" href=\"style/community.css\">";
        assert_eq!(INDEX.matches(link).count(), 1, "community.css is linked once");
        let community = INDEX.find(link).unwrap();
        let topbar = INDEX.find("<link data-trunk rel=\"css\" href=\"style/topbar.css\">").expect("topbar.css is linked");
        assert!(community < topbar, "community.css comes before topbar.css, which stays last");
        assert!(CSS.contains(".cm-hero") && CSS.contains(".cm-av img"));
        let av = &CSS[CSS.find(".cm-av {").unwrap()..];
        let av = &av[..av.find('}').unwrap()];
        assert!(av.contains("flex: none") && av.contains("aspect-ratio: 1 / 1"), "the avatar keeps its square: {av}");
    }

    #[test]
    fn the_old_pages_rules_are_gone_from_main_css() {
        // Written in two halves, so this test does not keep the classes alive for the patch that removes them.
        for gone in [
            concat!(".page-community-", "industrial"),
            concat!(".community-", "hero"),
            concat!(".community-", "glow"),
            concat!(".highlights-", "section"),
            concat!(".join-", "cta"),
            concat!(".ai-", "suggestion"),
        ] {
            assert!(!MAIN_CSS.contains(&format!("{gone} ")), "{gone} is still in main.css");
        }
    }

    #[test]
    fn the_hours_list_is_the_workers_ranking_without_empty_rows() {
        let entries = [row(1, "ada", 312.4), row(2, " ", 9.0), row(3, "bob", 4.5), row(4, "cyd", 0.0), row(5, "dee", 0.0)];
        let shown = ranked_hours(&entries, 10);
        assert_eq!(shown.iter().map(|e| (e.rank, e.username.as_str())).collect::<Vec<_>>(), [(1, "ada"), (3, "bob")]);
        assert_eq!(ranked_hours(&entries, 1).len(), 1);
        assert_eq!(hours_words(312.4), "312 h");
        assert_eq!(hours_words(1534.0), "1,534 h");
        assert_eq!(hours_words(4.5), "4.5 h");
        let html = render(full());
        assert!(html.contains("312 h") && html.contains("4.5 h"));
        assert!(!html.contains("@cyd"), "a row with no hours is left out");
        let only_zero = render(Data { board: Load::Ready(vec![row(1, "cyd", 0.0)]), ..empty() });
        assert!(only_zero.contains("No hours counted yet"));
    }

    #[test]
    fn each_creator_shows_their_own_newest_spaces_and_their_counts() {
        let list = [space("a", "ada"), space("b", "bob"), space("c", "Ada"), space("d", "ada")];
        assert_eq!(newest_by(&list, "ada", 2).iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["a", "c"]);
        assert!(newest_by(&list, "eve", 2).is_empty());
        assert_eq!(maker_line(&maker("ada", None)), "2 Spaces \u{00b7} 1,204 visits");
        assert_eq!(maker_line(&Maker { username: "x".to_string(), avatar_url: None, space_count: 1, visits: 1 }), "1 Space \u{00b7} 1 visit");
        let html = render(full());
        assert!(html.contains("href=\"/simulation/a\"") && html.contains("href=\"/simulation/c\""));
        assert!(html.contains("href=\"/profile/ada\""));
        // The newest Spaces are the Gallery's own cards.
        assert_eq!(html.matches("class=\"g-card").count(), 4);
        assert!(html.contains("g-live-card") && html.contains("2/8"));
    }

    #[test]
    fn search_waits_for_two_letters_and_sends_the_username_alone() {
        assert_eq!(search_ready("a"), None);
        assert_eq!(search_ready("  "), None);
        assert_eq!(search_ready("@a"), None);
        assert_eq!(search_ready(" @ada "), Some("ada".to_string()));
        assert_eq!(search_ready(&"x".repeat(80)).map(|q| q.len()), Some(64));
        assert_eq!(profile_href("a b"), "/profile/a%20b");
    }

    #[test]
    fn the_figures_are_only_what_is_known() {
        assert!(figures_of(None, &Load::Loading).is_empty());
        assert!(figures_of(Some(0), &Load::Failed).is_empty(), "a count of 0 is the Worker failing to count");
        assert_eq!(figures_of(Some(1534), &Load::Ready(live_one())), vec![("Members", "1,534".to_string()), ("Playing now", "2".to_string()), ("Live sessions", "1".to_string())]);
        assert_eq!(figures_of(None, &Load::Ready(LiveNow::default())), vec![("Playing now", "0".to_string()), ("Live sessions", "0".to_string())]);
        let html = render(full());
        assert!(html.contains("<dt>Members</dt><dd>1,534</dd>"));
        assert!(!render(all(|| Load::Failed)).contains("<dt>"));
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn the_community_address_shows_this_page() {
        let html = render_app_at("/community");
        assert!(html.contains("cm-page") && html.contains("The people building on Eustress"));
        assert!(!html.contains(concat!("page-community-", "industrial")));
    }
}
