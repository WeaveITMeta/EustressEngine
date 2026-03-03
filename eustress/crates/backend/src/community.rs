// =============================================================================
// Eustress Backend - Community API
// =============================================================================
// Table of Contents:
// 1. Imports
// 2. Request/Response Types
// 3. Handlers - search_users, get_leaderboard, get_community_stats, get_user_profile
// =============================================================================

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::db::UserResponse;
use crate::error::ApiError;
use crate::AppState;

// =============================================================================
// 2. Request/Response Types
// =============================================================================

/// Query parameters for user search.
#[derive(Debug, Deserialize)]
pub struct SearchUsersQuery {
    /// Partial username to search for.
    pub q: Option<String>,
    /// Maximum results to return (default 20, max 100).
    pub limit: Option<i64>,
    /// Offset for pagination (default 0).
    pub offset: Option<i64>,
}

/// Response for user search results.
#[derive(Debug, Serialize)]
pub struct SearchUsersResponse {
    pub users: Vec<UserResponse>,
    pub total: usize,
}

/// Query parameters for leaderboard.
#[derive(Debug, Deserialize)]
pub struct LeaderboardQuery {
    /// Maximum results to return (default 50, max 100).
    pub limit: Option<i64>,
    /// Offset for pagination (default 0).
    pub offset: Option<i64>,
}

/// A single leaderboard entry with rank information.
#[derive(Debug, Serialize)]
pub struct LeaderboardEntry {
    pub rank: usize,
    pub user: UserResponse,
}

/// Response for the leaderboard.
#[derive(Debug, Serialize)]
pub struct LeaderboardResponse {
    pub entries: Vec<LeaderboardEntry>,
}

/// Aggregate community statistics.
#[derive(Debug, Serialize)]
pub struct CommunityStatsResponse {
    pub total_users: i64,
    pub total_experiences: i64,
    pub total_plays: i64,
}

/// Public profile for a single user.
#[derive(Debug, Serialize)]
pub struct UserProfileResponse {
    pub user: UserResponse,
    pub experience_count: i64,
    pub total_plays: i64,
}

// =============================================================================
// 3. Handlers
// =============================================================================

/// Search users by username (public).
/// GET /api/community/search?q=<query>&limit=20&offset=0
pub async fn search_users(
    State(state): State<AppState>,
    Query(query): Query<SearchUsersQuery>,
) -> Result<Json<SearchUsersResponse>, ApiError> {
    let search_term = query.q.unwrap_or_default();
    let limit = query.limit.unwrap_or(20).min(100).max(1);
    let offset = query.offset.unwrap_or(0).max(0);
    
    let users = state.db.search_users(&search_term, limit, offset)
        .await
        .map_err(|e| ApiError::Database(e.to_string()))?;
    
    let total = users.len();
    let responses: Vec<UserResponse> = users.into_iter().map(Into::into).collect();
    
    Ok(Json(SearchUsersResponse {
        users: responses,
        total,
    }))
}

/// Get the bliss balance leaderboard (public).
/// GET /api/community/leaderboard?limit=50&offset=0
pub async fn get_leaderboard(
    State(state): State<AppState>,
    Query(query): Query<LeaderboardQuery>,
) -> Result<Json<LeaderboardResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).min(100).max(1);
    let offset = query.offset.unwrap_or(0).max(0);
    
    let users = state.db.get_leaderboard(limit, offset)
        .await
        .map_err(|e| ApiError::Database(e.to_string()))?;
    
    let entries: Vec<LeaderboardEntry> = users
        .into_iter()
        .enumerate()
        .map(|(index, user)| LeaderboardEntry {
            rank: (offset as usize) + index + 1,
            user: user.into(),
        })
        .collect();
    
    Ok(Json(LeaderboardResponse { entries }))
}

/// Get aggregate community statistics (public).
/// GET /api/community/stats
pub async fn get_community_stats(
    State(state): State<AppState>,
) -> Result<Json<CommunityStatsResponse>, ApiError> {
    let total_users = state.db.get_user_count()
        .await
        .map_err(|e| ApiError::Database(e.to_string()))?;
    
    let total_experiences = state.db.get_experience_count()
        .await
        .map_err(|e| ApiError::Database(e.to_string()))?;
    
    let total_plays = state.db.get_total_play_count()
        .await
        .map_err(|e| ApiError::Database(e.to_string()))?;
    
    Ok(Json(CommunityStatsResponse {
        total_users,
        total_experiences,
        total_plays,
    }))
}

/// Get a user's public profile by username (public).
/// GET /api/community/users/:username
pub async fn get_user_profile(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let user = state.db.find_user_by_username(&username)
        .await
        .map_err(|e| ApiError::Database(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound(format!("User '{}' not found", username)))?;
    
    let user_id = user.id.clone();
    let user_response: UserResponse = user.into();
    
    let experience_count = state.db.get_user_experience_count(&user_id)
        .await
        .map_err(|e| ApiError::Database(e.to_string()))?;
    
    let total_plays = state.db.get_user_total_plays(&user_id)
        .await
        .map_err(|e| ApiError::Database(e.to_string()))?;
    
    Ok(Json(UserProfileResponse {
        user: user_response,
        experience_count,
        total_plays,
    }))
}
