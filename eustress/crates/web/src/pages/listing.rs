// =============================================================================
// Eustress Web - A listing's page (/simulation/:id)
// =============================================================================
// One Space: its pictures, who made it, what it is, and the one Play area its state allows. Everything shown is what the Worker
// said about this listing:
//
//   GET /api/simulations/{id}            the listing (and, for its author, the review)
//   GET /api/simulations/{id}/live       is it hosted now
//   GET /api/simulations/{id}/previews   the captures Studio took at publish
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
use crate::components::play_chooser::{now_ms, status_of, watch, Facts, Live, Load, PlayChooserSlot, PlayStatePanel, Status, Viewer};
use crate::components::{CentralNav, Footer};
use crate::components::adult_content::{Age21ListingNotice, AdultListingNotice};
use crate::pages::review_panel::status_label;
use crate::state::{AppState, AuthState};
use crate::tags::gallery_href;

// -----------------------------------------------------------------------------
// 1. Wording
// -----------------------------------------------------------------------------

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
    let frames = {
        let listing = listing.clone();
        Memo::new(move |_| frames_of(&listing, &previews.get()))
    };
    let shot = RwSignal::new(0usize);

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

    let playing = move || match live.get() {
        Live::Hosted { players, .. } => players,
        _ => 0,
    };

    view! {
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
                        {move || {
                            let list = frames.get();
                            if list.is_empty() {
                                view! { <div class="g-frame-empty" aria-hidden="true"></div> }.into_any()
                            } else {
                                let count = list.len();
                                view! {
                                    {list.into_iter().enumerate().map(|(i, src)| view! {
                                        <img class="art-img" class:on=move || shot.get().min(count - 1) == i src=src alt="" decoding="async" />
                                    }).collect_view()}
                                    <div class="g-readout" aria-hidden="true"><span>{move || capture_words(shot.get(), count)}</span><span>"At publish"</span></div>
                                }.into_any()
                            }
                        }}
                    </div>
                    {move || {
                        let list = frames.get();
                        (list.len() > 1).then(|| view! {
                            <div class="g-caps" role="group" aria-label="Captures">
                                {list.into_iter().enumerate().map(|(i, src)| view! {
                                    <button type="button" class="g-cap" class:on=move || shot.get() == i aria-label=format!("Show capture {}", i + 1) aria-pressed=move || (shot.get() == i).to_string() on:click=move |_| shot.set(i)>
                                        <img src=src alt="" loading="lazy" decoding="async" />
                                    </button>
                                }).collect_view()}
                            </div>
                        })
                    }}
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
                    <h1 class="g-ltitle">{name.clone()}</h1>
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
                        <a class="g-btn" href="/downloads/player">"Download Eustress Player"</a>
                    </div>
                </aside>

                <div class="g-lcopy">
                    {(!description.is_empty()).then(|| view! {
                        <section class="g-csec"><h2>"About"</h2><p class="g-about">{description}</p></section>
                    })}
                    {move || {
                        let listed = matches!(status, Status::Listed | Status::Private);
                        match live.get() {
                            Live::Hosted { players, max_players, .. } if listed => {
                                let join_id = join_id.clone();
                                let pct = if max_players == 0 { 0 } else { (players.min(max_players) * 100 / max_players).min(100) };
                                view! {
                                    <section class="g-csec"><h2>"Live status"</h2>
                                        <div class="g-glass g-session">
                                            <span class="g-dot" aria-hidden="true"></span>
                                            <div><b>"Hosted by the author"</b><small>{format!("{players} of {max_players} players")}</small></div>
                                            <div class="g-meter" aria-hidden="true"><i style=format!("width:{pct}%")></i></div>
                                            {(viewer != Viewer::Phone).then(|| { let join_id = join_id.clone(); view! { <button type="button" class="g-btn g-btn-primary g-btn-sm" on:click=move |_| chooser.open(&join_id)>"Join"</button> } })}
                                        </div>
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
                            _ => ().into_any(),
                        }
                    }}
                    <section class="g-csec">
                        <h2>"Details"</h2>
                        <dl class="g-kv g-glass">
                            {details.into_iter().map(|(k, v)| view! { <dt>{k}</dt><dd>{v}</dd> }).collect_view()}
                        </dl>
                    </section>
                </div>
            </div>
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
