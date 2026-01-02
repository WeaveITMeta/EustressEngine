// =============================================================================
// Eustress Backend - Experiences API
// =============================================================================
// Endpoints for managing published experiences and favorites
// =============================================================================

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::{extract_token, validate_token};
use crate::db::Experience;
use crate::error::AppError;
use crate::AppState;

// =============================================================================
// Request/Response Types
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct SyncExperienceRequest {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub genre: String,
    pub max_players: i32,
    pub is_public: bool,
    pub allow_copying: bool,
    pub version: i32,
    pub changelog: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExperienceResponse {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub genre: String,
    pub max_players: i32,
    pub is_public: bool,
    pub allow_copying: bool,
    pub author_id: String,
    pub version: i32,
    pub play_count: i64,
    pub favorite_count: i64,
    pub created_at: DateTime<Utc>,
    pub published_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Experience> for ExperienceResponse {
    fn from(e: Experience) -> Self {
        Self {
            id: e.id,
            name: e.name,
            description: e.description,
            genre: e.genre,
            max_players: e.max_players,
            is_public: e.is_public,
            allow_copying: e.allow_copying,
            author_id: e.author_id,
            version: e.version,
            play_count: e.play_count,
            favorite_count: e.favorite_count,
            created_at: e.created_at,
            published_at: e.published_at,
            updated_at: e.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct FavoriteUpdatesQuery {
    pub since: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FavoriteUpdatesResponse {
    pub updates: Vec<ExperienceUpdate>,
}

#[derive(Debug, Serialize)]
pub struct ExperienceUpdate {
    pub experience_id: String,
    pub name: String,
    pub version: i32,
    pub updated_at: DateTime<Utc>,
    pub update_type: String, // "new_version", "new_content", etc.
}

// =============================================================================
// Handlers
// =============================================================================

/// Sync experience metadata from Worker to backend DB.
/// Called by the Worker after a successful publish commit.
pub async fn sync_experience(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SyncExperienceRequest>,
) -> Result<impl IntoResponse, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    let now = Utc::now();
    
    // Check if experience exists
    let existing = state.db.find_experience_by_id(&req.id).await.ok().flatten();
    
    let experience = Experience {
        id: req.id.clone(),
        name: req.name,
        description: req.description,
        genre: req.genre,
        max_players: req.max_players,
        is_public: req.is_public,
        allow_copying: req.allow_copying,
        author_id: claims.sub.clone(),
        version: req.version,
        play_count: existing.as_ref().map(|e| e.play_count).unwrap_or(0),
        favorite_count: existing.as_ref().map(|e| e.favorite_count).unwrap_or(0),
        created_at: existing.as_ref().map(|e| e.created_at).unwrap_or(now),
        published_at: now,
        updated_at: now,
    };
    
    // Upsert experience
    state.db.upsert_experience(&experience).await?;
    
    // Record version history
    if let Err(e) = state.db.record_experience_version(
        &req.id,
        req.version,
        req.changelog.as_deref(),
    ).await {
        tracing::warn!("Failed to record version: {}", e);
    }
    
    Ok(Json(serde_json::json!({
        "success": true,
        "experience_id": req.id,
        "version": req.version,
    })))
}

/// Get user's published experiences.
pub async fn get_my_experiences(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    let experiences = state.db.get_experiences_by_author(&claims.sub).await?;
    let responses: Vec<ExperienceResponse> = experiences.into_iter().map(Into::into).collect();
    Ok(Json(serde_json::json!({ "experiences": responses })))
}

/// Get a single experience by ID.
pub async fn get_experience(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.db.find_experience_by_id(&id).await {
        Ok(Some(experience)) => {
            let response: ExperienceResponse = experience.into();
            (StatusCode::OK, Json(serde_json::json!(response)))
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Experience not found" })),
        ),
        Err(e) => {
            tracing::error!("Failed to get experience: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Failed to fetch experience" })),
            )
        }
    }
}

// =============================================================================
// Favorites
// =============================================================================

/// Add experience to favorites.
pub async fn add_favorite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(experience_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    // Verify experience exists
    state.db.find_experience_by_id(&experience_id).await?
        .ok_or(AppError::NotFound)?;
    
    state.db.add_favorite(&claims.sub, &experience_id).await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

/// Remove experience from favorites.
pub async fn remove_favorite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(experience_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    state.db.remove_favorite(&claims.sub, &experience_id).await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

/// Get user's favorites.
pub async fn get_favorites(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    let favorites = state.db.get_user_favorites(&claims.sub).await?;
    
    // Get full experience details for each favorite
    let mut experiences = Vec::new();
    for fav in favorites {
        if let Ok(Some(exp)) = state.db.find_experience_by_id(&fav.experience_id).await {
            experiences.push(ExperienceResponse::from(exp));
        }
    }
    Ok(Json(serde_json::json!({ "favorites": experiences })))
}

/// Get updates to favorited experiences (for notifications).
pub async fn get_favorite_updates(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FavoriteUpdatesQuery>,
) -> Result<Json<FavoriteUpdatesResponse>, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    // Parse since timestamp, default to 24 hours ago
    let since = query.since
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|| Utc::now() - chrono::Duration::hours(24));
    
    let experiences = state.db.get_favorite_updates(&claims.sub, &since).await?;
    let updates: Vec<ExperienceUpdate> = experiences.into_iter().map(|e| {
        ExperienceUpdate {
            experience_id: e.id,
            name: e.name,
            version: e.version,
            updated_at: e.updated_at,
            update_type: "new_version".to_string(),
        }
    }).collect();
    
    Ok(Json(FavoriteUpdatesResponse { updates }))
}
