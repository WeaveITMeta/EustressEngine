// =============================================================================
// Eustress Web - The Gallery's catalog: listings, live sessions, makers, tags
// =============================================================================
// What the Gallery and a listing's page read from the Worker, and the small pure
// rules they share: which image a card shows, how a date reads ("published
// yesterday"), how a size and a rating read, which tags a card carries.
//
//   GET /api/simulations[?q=&tag=]          the listed listings, newest first (tag= repeats: a listing needs every one)
//   GET /api/simulations/{id}               one listing (and, for its author, its review)
//   GET /api/simulations/{id}/live          is it hosted now (src/live.mjs)
//   GET /api/simulations/{id}/previews      the review captures a card plays on hover
//   GET /api/simulations/live               every live session of a listed listing, with totals
//   GET /api/simulations/tags               the tags most used on listed listings
//   GET /api/makers?q=                      people with a listed listing
// A route a deployment does not have answers 404 or 503, and every fetch here turns that into
// "nothing", never into an error the page has to show.
// =============================================================================

use serde::Deserialize;

use super::{ApiClient, ApiError};
use crate::pages::review_panel::ReviewView;

// -----------------------------------------------------------------------------
// 1. What the Worker says
// -----------------------------------------------------------------------------

/// A cover image's size, for laying it out before it loads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Dimensions {
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
}

/// One listing, as the list and the single read carry it.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Listing {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The tags that describe it: the only category a listing has.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Who chose the tags: `"author"` or `"judge"` (the review). Absent means the author.
    #[serde(default)]
    pub tags_source: Option<String>,
    #[serde(default)]
    pub author_id: String,
    #[serde(default)]
    pub author_name: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    /// The creator's own cover (box art), when there is one.
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub cover: Option<Dimensions>,
    #[serde(default)]
    pub play_count: u64,
    #[serde(default)]
    pub favorite_count: u64,
    #[serde(default)]
    pub max_players: u32,
    #[serde(default)]
    pub rating: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub scene_size_bytes: Option<u64>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub featured: bool,
    /// Present, and false, only on a record that is not in the Gallery (its author's view).
    #[serde(default)]
    pub listable: Option<bool>,
    /// The author's review of a listing that is not in the Gallery.
    #[serde(default)]
    pub moderation: Option<ReviewView>,
}

#[derive(Debug, Deserialize)]
struct ListReply {
    #[serde(default)]
    simulations: Vec<Listing>,
}

/// One live session in `GET /api/simulations/live`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub author_name: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub players: u32,
    #[serde(default)]
    pub max_players: u32,
    /// The host's coarse region: `{ country, colo }`, or nothing.
    #[serde(default)]
    pub region: Option<Region>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Region {
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub colo: Option<String>,
}

/// Every live session of a listed listing, and the totals over all of them.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct LiveNow {
    #[serde(default)]
    pub sessions: Vec<Session>,
    #[serde(default)]
    pub players_total: u32,
    #[serde(default)]
    pub sessions_total: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct TagCount {
    pub tag: String,
    #[serde(default)]
    pub count: u32,
}

#[derive(Debug, Deserialize)]
struct TagsReply {
    #[serde(default)]
    tags: Vec<TagCount>,
}

/// A person with at least one listed listing.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Maker {
    pub username: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub space_count: u32,
    #[serde(default)]
    pub visits: u64,
}

#[derive(Debug, Deserialize)]
struct MakersReply {
    #[serde(default)]
    makers: Vec<Maker>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Preview {
    #[serde(default)]
    pub n: u32,
    pub url: String,
}

#[derive(Debug, Deserialize)]
struct PreviewsReply {
    #[serde(default)]
    previews: Vec<Preview>,
}

// -----------------------------------------------------------------------------
// 2. Fetches
// -----------------------------------------------------------------------------

/// Nothing is not an error: a route this deployment lacks, or a Worker that is busy, reads as an empty answer.
fn quiet<T: Default>(answer: Result<T, ApiError>) -> T {
    answer.unwrap_or_default()
}

pub async fn fetch_listings(client: &ApiClient) -> Result<Vec<Listing>, ApiError> {
    let reply: ListReply = client.get("/api/simulations").await?;
    Ok(reply.simulations)
}

pub async fn fetch_live_now(client: &ApiClient) -> LiveNow {
    quiet(client.get::<LiveNow>("/api/simulations/live").await)
}

pub async fn fetch_popular_tags(client: &ApiClient) -> Vec<TagCount> {
    quiet(client.get::<TagsReply>("/api/simulations/tags").await.map(|r| r.tags))
}

pub async fn search_makers(client: &ApiClient, query: &str) -> Vec<Maker> {
    let path = format!("/api/makers?limit=4&q={}", urlencoding::encode(query));
    quiet(client.get::<MakersReply>(&path).await.map(|r| r.makers))
}

pub async fn search_listings(client: &ApiClient, query: &str) -> Vec<Listing> {
    let path = format!("/api/simulations?q={}", urlencoding::encode(query));
    quiet(client.get::<ListReply>(&path).await.map(|r| r.simulations))
}

pub async fn fetch_previews(client: &ApiClient, id: &str) -> Vec<Preview> {
    quiet(client.get::<PreviewsReply>(&format!("/api/simulations/{id}/previews")).await.map(|r| r.previews))
}

// -----------------------------------------------------------------------------
// 3. Rules a card and a page share
// -----------------------------------------------------------------------------

/// Which kind of image a card shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtKind {
    /// The creator's own cover.
    Cover,
    /// The 16:9 capture Studio took at publish.
    Capture,
}

/// The image a card shows: the creator's cover when there is one, otherwise the capture. An address that is not an https
/// address on this deployment or a path is never used: it goes into an `<img>`.
pub fn art_of(listing: &Listing) -> Option<(ArtKind, String)> {
    let safe = |u: &Option<String>| u.clone().filter(|u| u.starts_with("https://") || u.starts_with('/'));
    safe(&listing.cover_url).map(|u| (ArtKind::Cover, u)).or_else(|| safe(&listing.thumbnail_url).map(|u| (ArtKind::Capture, u)))
}

/// A cover is portrait when it is taller than wide; the card then lays it out as box art.
pub fn cover_is_portrait(listing: &Listing) -> bool {
    listing.cover.is_some_and(|d| d.height > d.width)
}

/// The tags a card carries: the listing's own, and nothing else. A listing with none shows none.
pub fn card_tags(listing: &Listing) -> Vec<String> {
    listing.tags.clone()
}

/// "Suggested by review" for a listing whose tags the review chose, when the reader is its author; nothing for anyone else.
pub fn suggested_tags_label(listing: &Listing, reader_is_author: bool) -> Option<&'static str> {
    crate::tags::suggested_label(&listing.tags, listing.tags_source.as_deref(), reader_is_author)
}

pub fn is_multiplayer(listing: &Listing) -> bool {
    listing.max_players > 1
}

/// "Ages 13+" and its kin, as the review sets them, in the Worker's own words (moderation_email.mjs RATING_LABEL).
pub fn rating_words(rating: Option<&str>) -> Option<&'static str> {
    match rating {
        Some("all_ages") => Some("All ages"),
        Some("ages_10") => Some("Ages 10+"),
        Some("teen_13") => Some("Ages 13+"),
        Some("mature_17") => Some("Ages 17+"),
        Some("adult_18") => Some("Adults only (18+)"),
        Some("adult_21") => Some("Adults only (21+)"),
        _ => None,
    }
}

/// "2.4 MB", from a byte count.
pub fn size_words(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b >= KB * KB * KB {
        format!("{:.1} GB", b / (KB * KB * KB))
    } else if b >= KB * KB {
        format!("{:.1} MB", b / (KB * KB))
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

const DAY_MS: f64 = 86_400_000.0;

/// How long ago, for "new and rising": "published today", "published yesterday", "published 3 days ago", "published 2 weeks ago",
/// then the month and year. `now_ms` is the reader's clock; a time in the future reads as today.
pub fn published_words(iso: &str, now_ms: f64) -> Option<String> {
    let at = chrono::DateTime::parse_from_rfc3339(iso).ok()?.timestamp_millis() as f64;
    let days = ((now_ms - at) / DAY_MS).floor().max(0.0) as i64;
    Some(match days {
        0 => "published today".to_string(),
        1 => "published yesterday".to_string(),
        2..=13 => format!("published {days} days ago"),
        14..=59 => format!("published {} weeks ago", days / 7),
        _ => {
            let t = chrono::DateTime::from_timestamp_millis(at as i64)?;
            format!("published {}", t.format("%b %Y"))
        }
    })
}

/// Whether a listing was published within `days` of `now_ms`.
pub fn is_recent(listing: &Listing, days: f64, now_ms: f64) -> bool {
    listing
        .published_at
        .as_deref()
        .and_then(|iso| chrono::DateTime::parse_from_rfc3339(iso).ok())
        .is_some_and(|t| (now_ms - t.timestamp_millis() as f64) / DAY_MS <= days)
}

/// The listings, most visited first (ties: newest first), for a "busy this week" shelf and for picking what leads.
pub fn by_visits(listings: &[Listing]) -> Vec<Listing> {
    let mut out = listings.to_vec();
    out.sort_by(|a, b| b.play_count.cmp(&a.play_count).then_with(|| b.published_at.cmp(&a.published_at)));
    out
}

/// What leads the Gallery: the featured listings first, then by visits, at most `n`.
pub fn lead_listings(listings: &[Listing], n: usize) -> Vec<Listing> {
    let mut out = by_visits(listings);
    out.sort_by_key(|l| !l.featured);
    out.truncate(n);
    out
}

/// A listing's country and data-centre code as one short text for a live card: `US · PHX`, or just one of them.
pub fn region_words(region: Option<&Region>) -> Option<String> {
    let region = region?;
    let parts: Vec<&str> = [region.country.as_deref(), region.colo.as_deref()].into_iter().flatten().filter(|p| !p.is_empty()).collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(id: &str) -> Listing {
        Listing { id: id.to_string(), name: id.to_string(), ..Default::default() }
    }

    #[test]
    fn a_card_shows_the_creators_cover_and_otherwise_the_capture_and_never_an_address_it_should_not() {
        let mut l = listing("a");
        assert_eq!(art_of(&l), None);
        l.thumbnail_url = Some("https://api.example.dev/api/simulations/a/thumbnail".to_string());
        assert_eq!(art_of(&l).map(|a| a.0), Some(ArtKind::Capture));
        l.cover_url = Some("https://api.example.dev/api/simulations/a/cover?v=1".to_string());
        assert_eq!(art_of(&l).map(|a| a.0), Some(ArtKind::Cover));
        // An address that is not https or a path is skipped, and the next one is used.
        l.cover_url = Some("javascript:alert(1)".to_string());
        assert_eq!(art_of(&l).map(|a| a.0), Some(ArtKind::Capture));
        l.thumbnail_url = Some("http://insecure.example/x.png".to_string());
        assert_eq!(art_of(&l), None);
        l.thumbnail_url = Some("/assets/placeholder.png".to_string());
        assert_eq!(art_of(&l).map(|a| a.0), Some(ArtKind::Capture), "a path on this site is fine");
    }

    #[test]
    fn a_cover_taller_than_wide_is_box_art() {
        let mut l = listing("a");
        assert!(!cover_is_portrait(&l));
        l.cover = Some(Dimensions { width: 600, height: 800 });
        assert!(cover_is_portrait(&l));
        l.cover = Some(Dimensions { width: 1280, height: 720 });
        assert!(!cover_is_portrait(&l));
    }

    #[test]
    fn a_card_carries_the_listings_tags_and_nothing_else() {
        let mut l = listing("a");
        assert!(card_tags(&l).is_empty(), "no tags, none shown");
        l.tags = vec!["zombies".to_string(), "coop".to_string()];
        assert_eq!(card_tags(&l), vec!["zombies", "coop"]);
    }

    #[test]
    fn a_card_never_falls_back_to_a_genre_an_older_worker_still_sends() {
        for old in [
            "{\"id\":\"a\",\"genre\":\"Horror\",\"tags\":[]}",
            "{\"id\":\"a\",\"genre\":\"Sci Fi\"}",
            "{\"id\":\"a\",\"genre\":\"All\"}",
        ] {
            let l: Listing = serde_json::from_str(old).unwrap();
            assert!(card_tags(&l).is_empty(), "{old}");
        }
        let both: Listing = serde_json::from_str("{\"id\":\"a\",\"genre\":\"Horror\",\"tags\":[\"racing\"]}").unwrap();
        assert_eq!(card_tags(&both), vec!["racing"], "the tags, not the genre");
    }

    #[test]
    fn the_review_label_reads_tags_source_and_is_for_the_author_only() {
        let judged: Listing = serde_json::from_str("{\"id\":\"a\",\"tags\":[\"racing\"],\"tags_source\":\"judge\"}").unwrap();
        assert_eq!(suggested_tags_label(&judged, true), Some("Suggested by review"));
        assert_eq!(suggested_tags_label(&judged, false), None, "everyone else sees the tags without the label");
        let authored: Listing = serde_json::from_str("{\"id\":\"a\",\"tags\":[\"racing\"],\"tags_source\":\"author\"}").unwrap();
        assert_eq!(suggested_tags_label(&authored, true), None);
        let absent: Listing = serde_json::from_str("{\"id\":\"a\",\"tags\":[\"racing\"]}").unwrap();
        assert_eq!(absent.tags_source, None);
        assert_eq!(suggested_tags_label(&absent, true), None, "absent means the author");
        let null: Listing = serde_json::from_str("{\"id\":\"a\",\"tags\":[\"racing\"],\"tags_source\":null}").unwrap();
        assert_eq!(suggested_tags_label(&null, true), None);
    }

    #[test]
    fn ratings_sizes_and_player_counts_read_as_words() {
        assert_eq!(rating_words(Some("teen_13")), Some("Ages 13+"));
        assert_eq!(rating_words(Some("all_ages")), Some("All ages"));
        assert_eq!(rating_words(Some("adult_21")), Some("Adults only (21+)"));
        assert_eq!(rating_words(Some("ages_10")), Some("Ages 10+"));
        // the review panel says the same
        for r in ["all_ages", "ages_10", "teen_13", "mature_17", "adult_18", "adult_21"] {
            assert_eq!(rating_words(Some(r)), Some(crate::pages::review_panel::rating_label(r)), "{r}");
        }
        assert_eq!(rating_words(None), None);
        assert_eq!(rating_words(Some("something new")), None);
        assert_eq!(size_words(2_562_226), "2.4 MB");
        assert_eq!(size_words(900), "900 B");
        assert_eq!(size_words(52_000), "51 KB");
        assert_eq!(size_words(3 * 1024 * 1024 * 1024), "3.0 GB");
        let mut l = listing("a");
        assert!(!is_multiplayer(&l));
        l.max_players = 1;
        assert!(!is_multiplayer(&l), "one player is solo");
        l.max_players = 10;
        assert!(is_multiplayer(&l));
    }

    #[test]
    fn a_publish_date_reads_as_how_long_ago_in_the_readers_clock() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-10T12:00:00Z").unwrap().timestamp_millis() as f64;
        for (iso, want) in [
            ("2026-10-10T08:00:00Z", "published today"),
            ("2026-10-09T23:00:00Z", "published today"),
            ("2026-10-09T08:00:00Z", "published yesterday"),
            ("2026-10-07T12:00:00Z", "published 3 days ago"),
            ("2026-09-26T12:00:00Z", "published 2 weeks ago"),
            ("2026-07-01T12:00:00Z", "published Jul 2026"),
            ("2026-10-12T12:00:00Z", "published today"),
        ] {
            assert_eq!(published_words(iso, now).as_deref(), Some(want), "{iso}");
        }
        assert_eq!(published_words("not a time", now), None);
        let mut l = listing("a");
        l.published_at = Some("2026-10-05T12:00:00Z".to_string());
        assert!(is_recent(&l, 7.0, now));
        assert!(!is_recent(&l, 3.0, now));
        assert!(!is_recent(&listing("b"), 30.0, now), "no date, not recent");
    }

    #[test]
    fn what_leads_is_featured_first_then_the_most_visited_and_never_more_than_asked() {
        let mut a = listing("a");
        a.play_count = 5;
        let mut b = listing("b");
        b.play_count = 50;
        let mut c = listing("c");
        c.play_count = 1;
        c.featured = true;
        let all = vec![a.clone(), b.clone(), c.clone()];
        assert_eq!(by_visits(&all).iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["b", "a", "c"]);
        assert_eq!(lead_listings(&all, 3).iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["c", "b", "a"]);
        assert_eq!(lead_listings(&all, 1).iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["c"]);
        assert!(lead_listings(&[], 3).is_empty());
    }

    #[test]
    fn a_region_reads_as_country_and_data_centre_or_as_whichever_there_is() {
        assert_eq!(region_words(Some(&Region { country: Some("US".into()), colo: Some("PHX".into()) })).as_deref(), Some("US · PHX"));
        assert_eq!(region_words(Some(&Region { country: Some("US".into()), colo: None })).as_deref(), Some("US"));
        assert_eq!(region_words(Some(&Region { country: None, colo: None })), None);
        assert_eq!(region_words(None), None);
    }

    #[test]
    fn the_workers_answers_read_with_what_they_carry_and_without_what_an_older_worker_leaves_out() {
        let listing: Listing = serde_json::from_str(
            "{\"id\":\"07d0\",\"name\":\"Box Head\",\"genre\":\"Horror\",\"tags\":[],\"author_name\":\"Simbuilder\",\"thumbnail_url\":\"https://x/t\",\"play_count\":3,\"max_players\":10,\"rating\":\"teen_13\",\"scene_size_bytes\":2562226,\"format\":\"echk\"}",
        )
        .unwrap();
        assert_eq!((listing.name.as_str(), listing.play_count, listing.cover_url.clone()), ("Box Head", 3, None));
        let now: LiveNow = serde_json::from_str("{\"sessions\":[{\"id\":\"a\",\"name\":\"Box Head\",\"players\":2,\"max_players\":4,\"region\":{\"country\":\"US\",\"colo\":\"PHX\"}}],\"players_total\":2,\"sessions_total\":1}").unwrap();
        assert_eq!((now.players_total, now.sessions.len()), (2, 1));
        assert_eq!(region_words(now.sessions[0].region.as_ref()).as_deref(), Some("US · PHX"));
        let empty: LiveNow = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, LiveNow::default());
    }
}
