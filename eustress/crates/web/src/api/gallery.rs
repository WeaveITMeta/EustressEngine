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

// -----------------------------------------------------------------------------
// 1. Types
// -----------------------------------------------------------------------------

/// Gallery experience listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryExperience {
    pub id: String,
    pub name: String,
    pub description: String,
    pub thumbnail_url: Option<String>,
    pub creator_name: String,
    pub creator_id: String,
    pub visit_count: u64,
    pub like_count: u64,
    pub player_count: u32,
    pub rating: f32,
    pub genre: String,
    pub tags: Vec<String>,
    pub created_at: String,
}

/// Gallery query parameters.
#[derive(Debug, Default, Serialize)]
pub struct GalleryQuery {
    pub q: Option<String>,
    pub category: Option<String>,
    pub sort: Option<String>,
    pub genre: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    pub limit: Option<u32>,
}

// -----------------------------------------------------------------------------
// 2. Response Types
// -----------------------------------------------------------------------------

/// Response from get_gallery endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct GalleryResponse {
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

/// Get gallery experiences (paginated).
pub async fn get_gallery(
    client: &ApiClient,
    query: &GalleryQuery,
) -> Result<GalleryResponse, ApiError> {
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
    if let Some(g) = &query.genre {
        endpoint.push_str(&format!("genre={}&", g));
    }
    if let Some(p) = query.page {
        endpoint.push_str(&format!("page={}&", p));
    }
    if let Some(l) = query.limit {
        endpoint.push_str(&format!("limit={}&", l));
    }
    client.get(&endpoint).await
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
