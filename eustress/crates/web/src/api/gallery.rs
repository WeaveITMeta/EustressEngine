// =============================================================================
// Eustress Web - Gallery API
// =============================================================================
// Table of Contents:
// 1. Types
// 2. Response Types
// 3. Gallery API Functions
// =============================================================================

use serde::{Deserialize, Serialize};
use super::{ApiClient, ApiError};
use crate::tags::tag_query;

// -----------------------------------------------------------------------------
// 1. Types
// -----------------------------------------------------------------------------

/// Gallery simulation listing.
/// Accepts both frontend field names and API field names via serde aliases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryExperience {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(alias = "author_name", default)]
    pub creator_name: String,
    #[serde(alias = "author_id", default)]
    pub creator_id: String,
    #[serde(alias = "play_count", default)]
    pub visit_count: u64,
    #[serde(alias = "favorite_count", default)]
    pub like_count: u64,
    #[serde(alias = "max_players", default)]
    pub player_count: u32,
    /// Content rating as the Worker sends it: `"all_ages"`, `"teen_13"`,
    /// `"mature_17"`, `"adult_18"`, or `null` when unrated.
    ///
    /// This was typed `f32`, as though it were a star score. The Worker always
    /// emits the key (`m.rating || null`), and `#[serde(default)]` only covers an
    /// ABSENT field -- a present string fails as "expected f32", and a present
    /// `null` fails too on a non-Option. One bad card fails the whole `Vec`, so
    /// every Gallery response failed to parse.
    #[serde(default)]
    pub rating: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Who chose the tags: `"author"` or `"judge"`. Absent means the author.
    #[serde(default)]
    pub tags_source: Option<String>,
    #[serde(alias = "published_at", default)]
    pub created_at: String,
}

/// Gallery query parameters.
#[derive(Debug, Default, Serialize)]
pub struct GalleryQuery {
    pub q: Option<String>,
    pub category: Option<String>,
    pub sort: Option<String>,
    /// Every one of these tags (a listing needs all of them). Sent as repeated `tag=`.
    pub tags: Vec<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    pub limit: Option<u32>,
}

// -----------------------------------------------------------------------------
// 2. Response Types
// -----------------------------------------------------------------------------

/// Response from get_gallery endpoint.
/// API returns { simulations: [...] }, we alias to experiences for backward compat.
#[derive(Debug, Clone, Deserialize)]
pub struct GalleryResponse {
    #[serde(alias = "simulations")]
    pub experiences: Vec<GalleryExperience>,
}

/// Response from get_featured endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct FeaturedResponse {
    pub featured: Vec<GalleryExperience>,
}

// -----------------------------------------------------------------------------
// 3. Gallery API Functions
// -----------------------------------------------------------------------------

/// The path that asks the Worker for these gallery experiences: each tag as its own `tag=`.
pub fn gallery_endpoint(query: &GalleryQuery) -> String {
    let mut endpoint = "/api/gallery?".to_string();
    if let Some(q) = &query.q {
        endpoint.push_str(&format!("q={}&", q));
    }
    if let Some(c) = &query.category {
        endpoint.push_str(&format!("category={}&", c));
    }
    if let Some(s) = &query.sort {
        endpoint.push_str(&format!("sort={}&", s));
    }
    if !query.tags.is_empty() {
        endpoint.push_str(&tag_query(&query.tags));
        endpoint.push('&');
    }
    if let Some(p) = query.page {
        endpoint.push_str(&format!("page={}&", p));
    }
    if let Some(l) = query.limit {
        endpoint.push_str(&format!("limit={}&", l));
    }
    endpoint
}

/// Get gallery experiences (paginated).
pub async fn get_gallery(
    client: &ApiClient,
    query: &GalleryQuery,
) -> Result<GalleryResponse, ApiError> {
    client.get(&gallery_endpoint(query)).await
}

/// Get featured experiences.
pub async fn get_featured(client: &ApiClient) -> Result<FeaturedResponse, ApiError> {
    client.get("/api/gallery/featured").await
}

/// Get experience details by ID.
pub async fn get_experience_details(
    client: &ApiClient,
    experience_id: &str,
) -> Result<GalleryExperience, ApiError> {
    let endpoint = format!("/api/gallery/{}", experience_id);
    client.get(&endpoint).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gallery_path_sends_one_tag_per_tag_and_never_the_old_category_filter() {
        let mut q = GalleryQuery::default();
        assert_eq!(gallery_endpoint(&q), "/api/gallery?");
        q.tags = vec!["horror".to_string()];
        assert_eq!(gallery_endpoint(&q), "/api/gallery?tag=horror&");
        q.tags = vec!["horror".to_string(), "co-op".to_string()];
        q.sort = Some("new".to_string());
        q.limit = Some(20);
        assert_eq!(gallery_endpoint(&q), "/api/gallery?sort=new&tag=horror&tag=co-op&limit=20&");
        assert!(!gallery_endpoint(&q).contains("genre="));
    }

    #[test]
    fn an_answer_reads_with_tags_and_their_source_and_without_a_genre() {
        let one: GalleryExperience = serde_json::from_str("{\"id\":\"a\",\"name\":\"A\",\"tags\":[\"racing\"],\"tags_source\":\"judge\"}").unwrap();
        assert_eq!((one.tags, one.tags_source.as_deref()), (vec!["racing".to_string()], Some("judge")));
        let old: GalleryExperience = serde_json::from_str("{\"id\":\"a\",\"name\":\"A\",\"genre\":\"Horror\"}").unwrap();
        assert!(old.tags.is_empty() && old.tags_source.is_none(), "an older answer: the genre is ignored");
    }
}
