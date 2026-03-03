// =============================================================================
// Eustress Backend - Gallery API
// =============================================================================
// Table of Contents:
// 1. Imports
// 2. Request/Response Types
// 3. Handlers - get_gallery, get_featured, get_experience_details
// =============================================================================

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::AppState;

// =============================================================================
// 2. Request/Response Types
// =============================================================================

/// Query parameters for gallery listing.
#[derive(Debug, Deserialize)]
pub struct GalleryQuery {
    /// Filter by genre (e.g., "action", "rpg", "simulation").
    pub genre: Option<String>,
    /// Search by experience name.
    pub search: Option<String>,
    /// Maximum results to return (default 20, max 100).
    pub limit: Option<i64>,
    /// Offset for pagination (default 0).
    pub offset: Option<i64>,
}

/// A gallery listing item (summary view).
#[derive(Debug, Serialize)]
pub struct GalleryItem {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub genre: String,
    pub max_players: i32,
    pub author_id: String,
    pub author_name: Option<String>,
    pub play_count: i64,
    pub favorite_count: i64,
    pub published_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Response for paginated gallery listings.
#[derive(Debug, Serialize)]
pub struct GalleryResponse {
    pub experiences: Vec<GalleryItem>,
    pub count: usize,
}

/// Detailed experience view (includes version and author info).
#[derive(Debug, Serialize)]
pub struct ExperienceDetailResponse {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub genre: String,
    pub max_players: i32,
    pub is_public: bool,
    pub allow_copying: bool,
    pub author_id: String,
    pub author_name: Option<String>,
    pub version: i32,
    pub play_count: i64,
    pub favorite_count: i64,
    pub created_at: DateTime<Utc>,
    pub published_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// =============================================================================
// 3. Handlers
// =============================================================================

/// Get paginated gallery of public experiences (public).
/// GET /api/gallery?genre=action&search=cool&limit=20&offset=0
pub async fn get_gallery(
    State(state): State<AppState>,
    Query(query): Query<GalleryQuery>,
) -> Result<Json<GalleryResponse>, ApiError> {
    let limit = query.limit.unwrap_or(20).min(100).max(1);
    let offset = query.offset.unwrap_or(0).max(0);
    
    let experiences = state.db.get_public_experiences(
        query.genre.as_deref(),
        query.search.as_deref(),
        limit,
        offset,
    )
    .await
    .map_err(|e| ApiError::Database(e.to_string()))?;
    
    // Build gallery items with author names
    let mut items = Vec::with_capacity(experiences.len());
    for experience in experiences {
        let author_name = state.db.get_experience_author_name(&experience.author_id)
            .await
            .unwrap_or(None);
        
        items.push(GalleryItem {
            id: experience.id,
            name: experience.name,
            description: experience.description,
            genre: experience.genre,
            max_players: experience.max_players,
            author_id: experience.author_id,
            author_name,
            play_count: experience.play_count,
            favorite_count: experience.favorite_count,
            published_at: experience.published_at,
            updated_at: experience.updated_at,
        });
    }
    
    let count = items.len();
    Ok(Json(GalleryResponse {
        experiences: items,
        count,
    }))
}

/// Get featured experiences (public, ranked by popularity).
/// GET /api/gallery/featured
pub async fn get_featured(
    State(state): State<AppState>,
) -> Result<Json<GalleryResponse>, ApiError> {
    let experiences = state.db.get_featured_experiences(12)
        .await
        .map_err(|e| ApiError::Database(e.to_string()))?;
    
    let mut items = Vec::with_capacity(experiences.len());
    for experience in experiences {
        let author_name = state.db.get_experience_author_name(&experience.author_id)
            .await
            .unwrap_or(None);
        
        items.push(GalleryItem {
            id: experience.id,
            name: experience.name,
            description: experience.description,
            genre: experience.genre,
            max_players: experience.max_players,
            author_id: experience.author_id,
            author_name,
            play_count: experience.play_count,
            favorite_count: experience.favorite_count,
            published_at: experience.published_at,
            updated_at: experience.updated_at,
        });
    }
    
    let count = items.len();
    Ok(Json(GalleryResponse {
        experiences: items,
        count,
    }))
}

/// Get detailed experience information by ID (public).
/// GET /api/gallery/:id
pub async fn get_experience_details(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ExperienceDetailResponse>, ApiError> {
    let experience = state.db.find_experience_by_id(&id)
        .await
        .map_err(|e| ApiError::Database(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound(format!("Experience '{}' not found", id)))?;
    
    // Only show public experiences in gallery
    if !experience.is_public {
        return Err(ApiError::NotFound("Experience not found".into()));
    }
    
    let author_name = state.db.get_experience_author_name(&experience.author_id)
        .await
        .unwrap_or(None);
    
    Ok(Json(ExperienceDetailResponse {
        id: experience.id,
        name: experience.name,
        description: experience.description,
        genre: experience.genre,
        max_players: experience.max_players,
        is_public: experience.is_public,
        allow_copying: experience.allow_copying,
        author_id: experience.author_id,
        author_name,
        version: experience.version,
        play_count: experience.play_count,
        favorite_count: experience.favorite_count,
        created_at: experience.created_at,
        published_at: experience.published_at,
        updated_at: experience.updated_at,
    }))
}
